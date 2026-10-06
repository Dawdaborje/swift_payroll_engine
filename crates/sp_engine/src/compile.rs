use std::collections::{BTreeMap, BTreeSet};

use rust_decimal::Decimal;
use sp_expr::{compile, Expr, Value};

use crate::convert::value_from_json;
use crate::error::EngineError;
use crate::model::{Component, Rounding, Structure};

pub(crate) struct Item {
    pub(crate) component: Component,
    pub(crate) formula: Option<Expr>,
    pub(crate) fixed: Option<Decimal>,
    pub(crate) when: Option<Expr>,
    /// Its own tags and its kind.
    pub(crate) tags: Vec<String>,
    pub(crate) rounding: Rounding,
}

/// A structure that has been checked and put in calculation order. Make it once, use it for everyone.
pub struct Compiled {
    pub(crate) structure: Structure,
    pub(crate) items: Vec<Item>,
    pub(crate) order: Vec<usize>,
    pub(crate) constants: BTreeMap<String, Value>,
    pub(crate) inputs: Vec<(String, Option<Value>)>,
}

fn bad(message: impl Into<String>) -> EngineError {
    EngineError::Structure(message.into())
}

fn is_name(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c == '_') && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

const RESERVED: &[&str] = &["true", "false", "null", "in"];

impl Compiled {
    pub fn new(structure: &Structure) -> Result<Self, EngineError> {
        if structure.components.is_empty() {
            return Err(bad("it has no components"));
        }
        if structure.rounding.digits > 8 {
            return Err(bad("rounding keeps at most 8 digits"));
        }
        let mut names: BTreeSet<String> = BTreeSet::new();
        let mut claim = |name: &str, what: &str| -> Result<(), EngineError> {
            if !is_name(name) || RESERVED.contains(&name) {
                return Err(bad(format!("{what} `{name}` is not a name: lower-case letters, digits and _, not starting with a digit")));
            }
            if !names.insert(name.to_string()) {
                return Err(bad(format!("the name `{name}` is used twice (components, inputs and constants share one set of names)")));
            }
            Ok(())
        };
        let mut inputs = Vec::new();
        for spec in &structure.inputs {
            claim(&spec.name, "the input")?;
            let default = spec.default.as_ref().map(|d| value_from_json(d).map_err(|e| bad(format!("input `{}`: {e}", spec.name)))).transpose()?;
            inputs.push((spec.name.clone(), default));
        }
        let mut constants = BTreeMap::new();
        for (name, json) in &structure.constants {
            claim(name, "the constant")?;
            constants.insert(name.clone(), value_from_json(json).map_err(|e| bad(format!("constant `{name}`: {e}")))?);
        }
        for component in &structure.components {
            claim(&component.id, "the component")?;
        }

        let component_ids: BTreeSet<&str> = structure.components.iter().map(|c| c.id.as_str()).collect();
        let mut items = Vec::new();
        for component in &structure.components {
            let sources = [component.formula.is_some(), component.amount.is_some(), component.input.is_some()].iter().filter(|b| **b).count();
            if sources != 1 {
                return Err(bad(format!("`{}` needs exactly one of formula, amount and input", component.id)));
            }
            if let Some(input) = &component.input {
                if !structure.inputs.iter().any(|s| &s.name == input) {
                    return Err(bad(format!("`{}` reads the input `{input}`, which the structure does not declare", component.id)));
                }
            }
            let fixed = match &component.amount {
                Some(json) => match value_from_json(json).map_err(|e| bad(format!("`{}`: {e}", component.id)))? {
                    Value::Num(n) => Some(n),
                    _ => return Err(bad(format!("`{}`: amount is a number", component.id))),
                },
                None => None,
            };
            let formula = component.formula.as_deref().map(|f| compile(f).map_err(|e| bad(format!("`{}`: formula: {e}", component.id)))).transpose()?;
            let when = component.when.as_deref().map(|w| compile(w).map_err(|e| bad(format!("`{}`: when: {e}", component.id)))).transpose()?;
            let mut tags = vec![component.kind.tag().to_string()];
            for tag in &component.tags {
                if tag.trim().is_empty() {
                    return Err(bad(format!("`{}` has an empty tag", component.id)));
                }
                if !tags.contains(tag) {
                    tags.push(tag.clone());
                }
            }
            items.push(Item { component: component.clone(), formula, fixed, when, tags, rounding: component.round.unwrap_or(structure.rounding) });
        }

        let all_tags: BTreeSet<&str> = items.iter().flat_map(|i| i.tags.iter().map(String::as_str)).collect();
        // What each component needs to have been calculated first.
        let mut needs: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); items.len()];
        for (index, item) in items.iter().enumerate() {
            for expr in [item.formula.as_ref(), item.when.as_ref()].into_iter().flatten() {
                for name in expr.identifiers() {
                    if let Some(other) = items.iter().position(|i| i.component.id == name) {
                        if other == index {
                            return Err(bad(format!("`{}` refers to itself", item.component.id)));
                        }
                        needs[index].insert(other);
                    } else if !names.contains(&name) {
                        return Err(bad(format!("`{}` uses `{name}`, which is not a component, an input or a constant", item.component.id)));
                    }
                }
                for tag in expr.literal_arguments("total") {
                    if !all_tags.contains(tag.as_str()) {
                        return Err(bad(format!("`{}` totals the tag `{tag}`, which nothing has", item.component.id)));
                    }
                    for (other, candidate) in items.iter().enumerate() {
                        if candidate.tags.contains(&tag) {
                            if other == index {
                                return Err(bad(format!("`{}` totals `{tag}` and is tagged `{tag}` itself", item.component.id)));
                            }
                            needs[index].insert(other);
                        }
                    }
                }
            }
        }

        // The lowest-numbered component whose needs are done goes next, so the order stays the written one when it can.
        let mut order = Vec::with_capacity(items.len());
        let mut done = vec![false; items.len()];
        while order.len() < items.len() {
            let next = (0..items.len()).find(|i| !done[*i] && needs[*i].iter().all(|n| done[*n]));
            match next {
                Some(i) => {
                    done[i] = true;
                    order.push(i);
                }
                None => {
                    let stuck: Vec<&str> = (0..items.len()).filter(|i| !done[*i]).map(|i| items[i].component.id.as_str()).collect();
                    return Err(bad(format!("these components wait on each other: {}", stuck.join(", "))));
                }
            }
        }
        let _ = component_ids;
        Ok(Compiled { structure: structure.clone(), items, order, constants, inputs })
    }

    pub fn structure(&self) -> &Structure {
        &self.structure
    }

    /// The ids in the order they are calculated.
    pub fn calculation_order(&self) -> Vec<&str> {
        self.order.iter().map(|i| self.items[*i].component.id.as_str()).collect()
    }
}
