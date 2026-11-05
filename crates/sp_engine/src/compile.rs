use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap, HashMap};
use std::hash::{BuildHasherDefault, Hasher};

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

/// A small, fast hasher for the short names of components, inputs and constants (the standard one is
/// built to resist attackers and costs several times more per lookup, and a payroll does millions).
#[derive(Default)]
pub(crate) struct NameHasher(u64);

impl Hasher for NameHasher {
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.0 = (self.0.rotate_left(5) ^ u64::from_le_bytes(word)).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

pub(crate) type NameMap<V> = HashMap<String, V, BuildHasherDefault<NameHasher>>;

/// What a name in a formula stands for.
#[derive(Clone, Copy)]
pub(crate) enum Slot {
    /// A component (its position in the structure's list).
    Item(usize),
    /// An input (its position in `inputs`).
    Input(usize),
    /// A constant (its position in `constants`).
    Constant(usize),
}

/// A structure that has been checked and put in calculation order. Make it once, use it for everyone.
pub struct Compiled {
    pub(crate) structure: Structure,
    pub(crate) items: Vec<Item>,
    pub(crate) order: Vec<usize>,
    pub(crate) constants: Vec<Value>,
    pub(crate) inputs: Vec<(String, Option<Value>)>,
    pub(crate) slots: NameMap<Slot>,
    /// Every tag there is, and for each item the tags it adds to (as positions in `tag_names`).
    pub(crate) tag_names: NameMap<usize>,
    pub(crate) item_tags: Vec<Vec<usize>>,
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
        let mut constants = Vec::new();
        let mut slots: NameMap<Slot> = NameMap::default();
        for (position, spec) in structure.inputs.iter().enumerate() {
            slots.insert(spec.name.clone(), Slot::Input(position));
        }
        for (name, json) in &structure.constants {
            claim(name, "the constant")?;
            slots.insert(name.clone(), Slot::Constant(constants.len()));
            constants.push(value_from_json(json).map_err(|e| bad(format!("constant `{name}`: {e}")))?);
        }
        for (position, component) in structure.components.iter().enumerate() {
            claim(&component.id, "the component")?;
            slots.insert(component.id.clone(), Slot::Item(position));
        }

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

        // Tags: who has each one (the members) and who totals it (the readers).
        let mut tag_names: NameMap<usize> = NameMap::default();
        let mut item_tags: Vec<Vec<usize>> = Vec::with_capacity(items.len());
        let mut members: Vec<Vec<usize>> = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let mut mine = Vec::with_capacity(item.tags.len());
            for tag in &item.tags {
                let next = tag_names.len();
                let id = *tag_names.entry(tag.clone()).or_insert(next);
                if id == members.len() {
                    members.push(Vec::new());
                }
                members[id].push(index);
                mine.push(id);
            }
            item_tags.push(mine);
        }

        // What each component needs: other components by name, and every member of a tag it totals.
        // A tag is one shared stop on the way (a node of its own), so a total over thousands of members
        // costs one edge per reader, not one per member.
        let mut needs_items: Vec<Vec<usize>> = vec![Vec::new(); items.len()];
        let mut needs_tags: Vec<Vec<usize>> = vec![Vec::new(); items.len()];
        for (index, item) in items.iter().enumerate() {
            for expr in [item.formula.as_ref(), item.when.as_ref()].into_iter().flatten() {
                for name in expr.identifiers() {
                    match slots.get(&name) {
                        Some(Slot::Item(other)) => {
                            if *other == index {
                                return Err(bad(format!("`{}` refers to itself", item.component.id)));
                            }
                            needs_items[index].push(*other);
                        }
                        Some(_) => {}
                        None => return Err(bad(format!("`{}` uses `{name}`, which is not a component, an input or a constant", item.component.id))),
                    }
                }
                for tag in expr.literal_arguments("total") {
                    let Some(&id) = tag_names.get(&tag) else {
                        return Err(bad(format!("`{}` totals the tag `{tag}`, which nothing has", item.component.id)));
                    };
                    if members[id].contains(&index) {
                        return Err(bad(format!("`{}` totals `{tag}` and is tagged `{tag}` itself", item.component.id)));
                    }
                    needs_tags[index].push(id);
                }
            }
            needs_items[index].sort_unstable();
            needs_items[index].dedup();
            needs_tags[index].sort_unstable();
            needs_tags[index].dedup();
        }

        // The lowest-numbered component whose needs are done goes next, so the order stays the written one
        // when it can (a min-heap of the components that are ready).
        let mut waiting: Vec<usize> = (0..items.len()).map(|i| needs_items[i].len() + needs_tags[i].len()).collect();
        let mut tag_waiting: Vec<usize> = members.iter().map(Vec::len).collect();
        let mut item_readers: Vec<Vec<usize>> = vec![Vec::new(); items.len()];
        let mut tag_readers: Vec<Vec<usize>> = vec![Vec::new(); members.len()];
        for index in 0..items.len() {
            for &other in &needs_items[index] {
                item_readers[other].push(index);
            }
            for &tag in &needs_tags[index] {
                tag_readers[tag].push(index);
            }
        }
        let mut ready: BinaryHeap<Reverse<usize>> = (0..items.len()).filter(|i| waiting[*i] == 0).map(Reverse).collect();
        let mut order = Vec::with_capacity(items.len());
        while let Some(Reverse(next)) = ready.pop() {
            order.push(next);
            let mut freed: Vec<usize> = Vec::new();
            freed.extend(item_readers[next].iter().copied());
            for &tag in &item_tags[next] {
                tag_waiting[tag] -= 1;
                if tag_waiting[tag] == 0 {
                    freed.extend(tag_readers[tag].iter().copied());
                }
            }
            for reader in freed {
                waiting[reader] -= 1;
                if waiting[reader] == 0 {
                    ready.push(Reverse(reader));
                }
            }
        }
        if order.len() < items.len() {
            let mut done = vec![false; items.len()];
            order.iter().for_each(|i| done[*i] = true);
            let stuck: Vec<&str> = (0..items.len()).filter(|i| !done[*i]).map(|i| items[i].component.id.as_str()).collect();
            let shown = stuck.iter().take(20).copied().collect::<Vec<_>>().join(", ");
            let more = if stuck.len() > 20 { format!(" and {} more", stuck.len() - 20) } else { String::new() };
            return Err(bad(format!("these components wait on each other: {shown}{more}")));
        }
        Ok(Compiled { structure: structure.clone(), items, order, constants, inputs, slots, tag_names, item_tags })
    }

    pub fn structure(&self) -> &Structure {
        &self.structure
    }

    /// The ids in the order they are calculated.
    pub fn calculation_order(&self) -> Vec<&str> {
        self.order.iter().map(|i| self.items[*i].component.id.as_str()).collect()
    }
}
