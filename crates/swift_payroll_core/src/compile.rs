use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap, HashMap};
use std::fmt;
use std::hash::{BuildHasherDefault, Hasher};

use rust_decimal::Decimal;
use swift_payroll_expr::{builtins::check_bracket_table, compile, Expr, Precision, Value};

use crate::convert::{number_from_text, value_from_json};
use crate::error::EngineError;
use crate::model::{Component, InputType, Rounding, Structure};

pub(crate) struct Item {
    pub(crate) component: Component,
    pub(crate) formula: Option<Expr>,
    pub(crate) fixed: Option<Decimal>,
    pub(crate) when: Option<Expr>,
    /// Its own tags and its kind.
    pub(crate) tags: Vec<String>,
    pub(crate) rounding: Rounding,
    /// For an `input` component: the input's position.
    pub(crate) input: Option<usize>,
}

/// A small, fast hasher for the short names of components, inputs and constants (the standard one is
/// built to resist attackers and costs several times more per lookup, and a payroll does millions).
///
/// Only names from the structure are ever stored with it, and a person's unknown input names are refused
/// at the first miss, so a person cannot slow a payroll down with colliding names. A *structure* written to
/// collide can make [`Compiled::new`] slow (quadratic in its number of names): a host that compiles
/// structures from people it does not trust should limit their size.
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

/// A declared input: its name, default, and type.
pub(crate) struct InputSlot {
    pub(crate) name: String,
    pub(crate) default: Option<Value>,
    pub(crate) input_type: InputType,
}

/// Something in a structure that is allowed but probably a mistake.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Warning {
    /// An input nothing reads: no formula, no condition and no `input` component.
    UnusedInput(String),
    /// A constant no formula or condition reads.
    UnusedConstant(String),
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Warning::UnusedInput(name) => write!(f, "the input `{name}` is declared but nothing reads it"),
            Warning::UnusedConstant(name) => write!(f, "the constant `{name}` is declared but nothing reads it"),
        }
    }
}

/// A structure that has been checked and put in calculation order. Make it once, use it for everyone
/// (it is `Send` and `Sync`, so threads can share one).
pub struct Compiled {
    pub(crate) structure: Structure,
    pub(crate) items: Vec<Item>,
    pub(crate) order: Vec<usize>,
    pub(crate) constants: Vec<Value>,
    pub(crate) constant_names: Vec<String>,
    pub(crate) inputs: Vec<InputSlot>,
    pub(crate) slots: NameMap<Slot>,
    /// Every tag there is, and for each item the tags it adds to (as positions in `tag_names`).
    pub(crate) tag_names: NameMap<usize>,
    pub(crate) item_tags: Vec<Vec<usize>>,
    /// Deductions in the order they are taken when net pay is held at zero.
    pub(crate) deduction_order: Vec<usize>,
    pub(crate) precision: Precision,
    warnings: Vec<Warning>,
}

impl fmt::Debug for Compiled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Compiled")
            .field("structure", &self.structure.id)
            .field("version", &self.structure.version)
            .field("order", &self.calculation_order())
            .finish_non_exhaustive()
    }
}

fn bad(message: impl Into<String>) -> EngineError {
    EngineError::Structure { component: None, message: message.into() }
}

fn bad_in(component: &str, message: impl Into<String>) -> EngineError {
    EngineError::Structure { component: Some(component.to_string()), message: message.into() }
}

fn is_name(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c == '_') && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

const RESERVED: &[&str] = &["true", "false", "null", "in"];

/// Whether a value has an input's type (numbers given as decimal text count as numbers).
pub(crate) fn typed(value: Value, input_type: InputType) -> Result<Value, String> {
    match (input_type, value) {
        (InputType::Number, Value::Num(n)) => Ok(Value::Num(n)),
        (InputType::Number, Value::Str(text)) => number_from_text(&text).map(Value::Num),
        (InputType::Text, Value::Str(text)) => Ok(Value::Str(text)),
        (InputType::Bool, Value::Bool(b)) => Ok(Value::Bool(b)),
        (InputType::Any, value) => Ok(value),
        (wanted, other) => Err(format!(
            "is a {}, not {}",
            other.type_name(),
            match wanted {
                InputType::Number => "a number",
                InputType::Text => "text",
                InputType::Bool => "true or false",
                _ => "of the declared type",
            }
        )),
    }
}

impl Compiled {
    /// Check a structure and find the order its components are calculated in. A loop, an unknown name, a
    /// typo in a formula, a broken bracket table or an impossible component is refused here, before any
    /// payroll runs.
    pub fn new(structure: &Structure) -> Result<Self, EngineError> {
        if structure.components.is_empty() {
            return Err(bad("it has no components"));
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
            let default = match &spec.default {
                Some(json) => {
                    let value = value_from_json(json).map_err(|e| bad(format!("input `{}`: {e}", spec.name)))?;
                    Some(typed(value, spec.input_type).map_err(|e| bad(format!("the default of input `{}` {e}", spec.name)))?)
                }
                None => None,
            };
            inputs.push(InputSlot { name: spec.name.clone(), default, input_type: spec.input_type });
        }
        let mut constants = Vec::new();
        let mut constant_names = Vec::new();
        let mut slots: NameMap<Slot> = NameMap::default();
        for (position, spec) in structure.inputs.iter().enumerate() {
            slots.insert(spec.name.clone(), Slot::Input(position));
        }
        for (name, json) in &structure.constants {
            claim(name, "the constant")?;
            slots.insert(name.clone(), Slot::Constant(constants.len()));
            constants.push(value_from_json(json).map_err(|e| bad(format!("constant `{name}`: {e}")))?);
            constant_names.push(name.clone());
        }
        for (position, component) in structure.components.iter().enumerate() {
            claim(&component.id, "the component")?;
            slots.insert(component.id.clone(), Slot::Item(position));
        }

        let mut items = Vec::new();
        for component in &structure.components {
            let id = component.id.as_str();
            let sources = [component.formula.is_some(), component.amount.is_some(), component.input.is_some()].iter().filter(|b| **b).count();
            if sources != 1 {
                return Err(bad_in(id, format!("`{id}` needs exactly one of formula, amount and input")));
            }
            let input = match &component.input {
                Some(input) => {
                    let position = structure
                        .inputs
                        .iter()
                        .position(|s| &s.name == input)
                        .ok_or_else(|| bad_in(id, format!("`{id}` reads the input `{input}`, which the structure does not declare")))?;
                    let input_type = structure.inputs[position].input_type;
                    if !matches!(input_type, InputType::Any | InputType::Number) {
                        return Err(bad_in(id, format!("`{id}` reads the input `{input}`, which is declared as something other than a number")));
                    }
                    Some(position)
                }
                None => None,
            };
            let fixed = match &component.amount {
                Some(json) => match value_from_json(json).map_err(|e| bad_in(id, format!("`{id}`: {e}")))? {
                    Value::Num(n) => Some(n),
                    Value::Str(text) => Some(number_from_text(&text).map_err(|e| bad_in(id, format!("`{id}`: amount {e}")))?),
                    _ => return Err(bad_in(id, format!("`{id}`: amount is a number"))),
                },
                None => None,
            };
            let formula = component.formula.as_deref().map(|f| compile(f).map_err(|e| bad_in(id, format!("`{id}`: formula: {e}")))).transpose()?;
            let when = component.when.as_deref().map(|w| compile(w).map_err(|e| bad_in(id, format!("`{id}`: when: {e}")))).transpose()?;
            let mut tags = vec![component.kind.tag().to_string()];
            for tag in &component.tags {
                if tag.trim().is_empty() {
                    return Err(bad_in(id, format!("`{id}` has an empty tag")));
                }
                if !tags.contains(tag) {
                    tags.push(tag.clone());
                }
            }
            items.push(Item { component: component.clone(), formula, fixed, when, tags, rounding: component.round.unwrap_or(structure.rounding), input });
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
        let mut read: BTreeSet<String> = BTreeSet::new();
        for (index, item) in items.iter().enumerate() {
            let id = item.component.id.as_str();
            if let Some(input) = &item.component.input {
                read.insert(input.clone());
            }
            for expr in [item.formula.as_ref(), item.when.as_ref()].into_iter().flatten() {
                if !expr.calls_match_literals("total", 1, &[0]) {
                    return Err(bad_in(id, format!("`{id}` calls total without exactly one literal tag string")));
                }
                for name in expr.identifiers() {
                    match slots.get(&name) {
                        Some(Slot::Item(other)) => {
                            if *other == index {
                                return Err(bad_in(id, format!("`{id}` refers to itself")));
                            }
                            needs_items[index].push(*other);
                        }
                        Some(_) => {}
                        None => return Err(bad_in(id, format!("`{id}` uses `{name}`, which is not a component, an input or a constant"))),
                    }
                    read.insert(name);
                }
                // A table named in `bracket` or `rate_at` is checked now, not when the first person meets it.
                for function in ["bracket", "rate_at"] {
                    for table in expr.identifier_arguments(function, 1) {
                        match slots.get(&table) {
                            Some(Slot::Constant(position)) => {
                                check_bracket_table(&constants[*position]).map_err(|e| bad_in(id, format!("`{id}`: the table `{table}`: {e}")))?;
                            }
                            Some(Slot::Item(_)) => return Err(bad_in(id, format!("`{id}`: `{table}` is a component (a number), not a bracket table"))),
                            _ => {}
                        }
                    }
                }
                for tag in expr.literal_arguments("total") {
                    let Some(&tag_id) = tag_names.get(&tag) else {
                        return Err(bad_in(id, format!("`{id}` totals the tag `{tag}`, which nothing has")));
                    };
                    if members[tag_id].contains(&index) {
                        return Err(bad_in(id, format!("`{id}` totals `{tag}` and is tagged `{tag}` itself")));
                    }
                    needs_tags[index].push(tag_id);
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

        let mut deduction_order: Vec<usize> = (0..items.len()).filter(|i| items[*i].component.kind == crate::Kind::Deduction).collect();
        deduction_order.sort_by_key(|i| (items[*i].component.priority.unwrap_or(u32::MAX), *i));

        let mut warnings = Vec::new();
        warnings.extend(structure.inputs.iter().filter(|s| !read.contains(&s.name)).map(|s| Warning::UnusedInput(s.name.clone())));
        warnings.extend(structure.constants.keys().filter(|name| !read.contains(*name)).map(|name| Warning::UnusedConstant(name.clone())));

        Ok(Compiled {
            structure: structure.clone(),
            items,
            order,
            constants,
            constant_names,
            inputs,
            slots,
            tag_names,
            item_tags,
            deduction_order,
            precision: structure.precision,
            warnings,
        })
    }

    /// The structure it was made from.
    pub fn structure(&self) -> &Structure {
        &self.structure
    }

    /// The ids in the order they are calculated.
    pub fn calculation_order(&self) -> Vec<&str> {
        self.order.iter().map(|i| self.items[*i].component.id.as_str()).collect()
    }

    /// What in the structure is allowed but probably a mistake (an input or constant nothing reads).
    /// A host should show these to whoever wrote the structure.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }
}
