use std::borrow::Cow;
use std::collections::BTreeMap;

use rust_decimal::Decimal;

use crate::ast::{Node, Op};
use crate::builtins;
use crate::error::{Error, Result};
use crate::value::Value;

/// A backstop: the parser already bounds how deep a tree can be (see `parse.rs`), well below this.
const MAX_DEPTH: usize = 2048;

/// What an expression can see: names, and functions the host adds to the built-in ones.
pub trait Env {
    fn get(&self, name: &str) -> Option<Value>;

    /// Like [`get`](Env::get), but a host that keeps its values can lend them instead of copying: a table
    /// of bands read by every formula for every person is then never cloned. The default copies.
    fn get_ref(&self, name: &str) -> Option<Cow<'_, Value>> {
        self.get(name).map(Cow::Owned)
    }

    /// A host function. `None` means "not mine": the built-ins are tried next.
    fn call(&self, _name: &str, _args: &[&Value]) -> Option<Result<Value>> {
        None
    }
}

/// An environment that is only a map of names.
#[derive(Debug, Clone, Default)]
pub struct MapEnv(pub BTreeMap<String, Value>);

impl Env for MapEnv {
    fn get(&self, name: &str) -> Option<Value> {
        self.0.get(name).cloned()
    }
}

fn too_big(at: usize) -> Error {
    Error::at("the number is too large", at)
}

fn truth(value: &Value, at: Option<usize>) -> Result<bool> {
    match value {
        Value::Bool(b) => Ok(*b),
        other => Err(Error { message: format!("a condition is true or false, not a {}", other.type_name()), at }),
    }
}

/// Evaluate to a value that is borrowed when it already exists somewhere (a literal in the expression, a
/// name in the environment, a member of a map) and built only when something was calculated.
pub(crate) fn eval<'a>(node: &'a Node, env: &'a dyn Env, depth: usize) -> Result<Cow<'a, Value>> {
    if depth > MAX_DEPTH {
        return Err(Error::new("the expression is nested too deeply"));
    }
    let next = depth + 1;
    match node {
        Node::Lit(value) => Ok(Cow::Borrowed(value)),
        Node::Ident(name, at) => env.get_ref(name).ok_or_else(|| Error::at(format!("`{name}` is not defined"), *at)),
        Node::List(items) => Ok(Cow::Owned(Value::List(items.iter().map(|n| eval(n, env, next).map(Cow::into_owned)).collect::<Result<_>>()?))),
        Node::Map(entries) => {
            let mut map = BTreeMap::new();
            for (key, value) in entries {
                match eval(key, env, next)?.into_owned() {
                    Value::Str(key) => {
                        map.insert(key, eval(value, env, next)?.into_owned());
                    }
                    other => return Err(Error::new(format!("a map key is text, not a {}", other.type_name()))),
                }
            }
            Ok(Cow::Owned(Value::Map(map)))
        }
        Node::Member(inner, key, at) => match eval(inner, env, next)? {
            Cow::Borrowed(Value::Map(map)) => map.get(key).map(Cow::Borrowed).ok_or_else(|| Error::at(format!("there is no `{key}`"), *at)),
            Cow::Owned(Value::Map(mut map)) => map.remove(key).map(Cow::Owned).ok_or_else(|| Error::at(format!("there is no `{key}`"), *at)),
            other => Err(Error::at(format!("`.{key}` needs a map, not a {}", other.type_name()), *at)),
        },
        Node::Index(inner, index, at) => {
            let container = eval(inner, env, next)?;
            let index = eval(index, env, next)?;
            match (&*container, &*index) {
                (Value::List(items), Value::Num(n)) => {
                    let i = usize::try_from(i64::try_from(n.trunc()).map_err(|_| Error::at("a list position is a whole number", *at))?).map_err(|_| Error::at("a list position is not negative", *at))?;
                    let found = items.get(i).ok_or_else(|| Error::at(format!("there is no item {i}"), *at))?;
                    Ok(Cow::Owned(found.clone()))
                }
                (Value::Map(map), Value::Str(key)) => {
                    let found = map.get(key).ok_or_else(|| Error::at(format!("there is no `{key}`"), *at))?;
                    Ok(Cow::Owned(found.clone()))
                }
                (a, b) => Err(Error::at(format!("cannot index a {} by a {}", a.type_name(), b.type_name()), *at)),
            }
        }
        Node::Call(name, args, at) => {
            // Most calls have a few arguments: keep them on the stack.
            let mut stack: [Cow<'a, Value>; 4] = [Cow::Borrowed(&NULL), Cow::Borrowed(&NULL), Cow::Borrowed(&NULL), Cow::Borrowed(&NULL)];
            let mut heap: Vec<Cow<'a, Value>> = Vec::new();
            let values: &[Cow<'a, Value>] = if args.len() <= stack.len() {
                for (slot, arg) in stack.iter_mut().zip(args) {
                    *slot = eval(arg, env, next)?;
                }
                &stack[..args.len()]
            } else {
                for arg in args {
                    heap.push(eval(arg, env, next)?);
                }
                &heap
            };
            let mut small: [&Value; 4] = [&NULL; 4];
            let large: Vec<&Value>;
            let refs: &[&Value] = if values.len() <= small.len() {
                for (slot, value) in small.iter_mut().zip(values) {
                    *slot = &**value;
                }
                &small[..values.len()]
            } else {
                large = values.iter().map(|v| &**v).collect();
                &large
            };
            let result = match env.call(name, refs) {
                Some(result) => result,
                None => builtins::call(name, refs),
            };
            result.map(Cow::Owned).map_err(|e| Error { message: format!("{name}: {}", e.message), at: e.at.or(Some(*at)) })
        }
        Node::Not(inner, at) => Ok(Cow::Owned(Value::Bool(!truth(&*eval(inner, env, next)?, Some(*at))?))),
        Node::Neg(inner, at) => match &*eval(inner, env, next)? {
            Value::Num(n) => Ok(Cow::Owned(Value::Num(-*n))),
            other => Err(Error::at(format!("cannot negate a {}", other.type_name()), *at)),
        },
        Node::And(items) => {
            for item in items {
                if !truth(&*eval(item, env, next)?, None)? {
                    return Ok(Cow::Owned(Value::Bool(false)));
                }
            }
            Ok(Cow::Owned(Value::Bool(true)))
        }
        Node::Or(items) => {
            for item in items {
                if truth(&*eval(item, env, next)?, None)? {
                    return Ok(Cow::Owned(Value::Bool(true)));
                }
            }
            Ok(Cow::Owned(Value::Bool(false)))
        }
        Node::Cond(c, yes, no) => {
            if truth(&*eval(c, env, next)?, None)? {
                eval(yes, env, next)
            } else {
                eval(no, env, next)
            }
        }
        Node::Chain(first, rest) => {
            let mut acc = eval(first, env, next)?;
            for (op, node, at) in rest {
                let right = eval(node, env, next)?;
                acc = Cow::Owned(binary(*op, acc, right, *at)?);
            }
            Ok(acc)
        }
    }
}

static NULL: Value = Value::Null;

fn binary(op: Op, a: Cow<'_, Value>, b: Cow<'_, Value>, at: usize) -> Result<Value> {
    use Value::*;
    // Numbers are by far the commonest operands: handle them without ever copying anything.
    if let (Num(x), Num(y)) = (&*a, &*b) {
        let (x, y) = (*x, *y);
        return match op {
            Op::Add => x.checked_add(y).map(Num).ok_or_else(|| too_big(at)),
            Op::Sub => x.checked_sub(y).map(Num).ok_or_else(|| too_big(at)),
            Op::Mul => x.checked_mul(y).map(Num).ok_or_else(|| too_big(at)),
            Op::Div if y.is_zero() => Err(Error::at("division by zero", at)),
            Op::Div => x.checked_div(y).map(Num).ok_or_else(|| too_big(at)),
            Op::Rem if y.is_zero() => Err(Error::at("division by zero", at)),
            Op::Rem => x.checked_rem(y).map(Num).ok_or_else(|| too_big(at)),
            Op::Eq => Ok(Bool(x == y)),
            Op::Ne => Ok(Bool(x != y)),
            Op::Lt | Op::Le | Op::Gt | Op::Ge => Ok(Bool(compare(op, x.cmp(&y)))),
            Op::In => Err(Error::at(format!("cannot use `{}` on a number and a number", symbol(op)), at)),
        };
    }
    match (op, a.into_owned(), b.into_owned()) {
        (Op::Add, Num(x), Num(y)) => x.checked_add(y).map(Num).ok_or_else(|| too_big(at)),
        (Op::Sub, Num(x), Num(y)) => x.checked_sub(y).map(Num).ok_or_else(|| too_big(at)),
        (Op::Mul, Num(x), Num(y)) => x.checked_mul(y).map(Num).ok_or_else(|| too_big(at)),
        (Op::Div, Num(x), Num(y)) => {
            if y.is_zero() {
                return Err(Error::at("division by zero", at));
            }
            x.checked_div(y).map(Num).ok_or_else(|| too_big(at))
        }
        (Op::Rem, Num(x), Num(y)) => {
            if y.is_zero() {
                return Err(Error::at("division by zero", at));
            }
            x.checked_rem(y).map(Num).ok_or_else(|| too_big(at))
        }
        (Op::Add, Str(x), Str(y)) => Ok(Str(x + &y)),
        (Op::Add, List(mut x), List(y)) => {
            x.extend(y);
            Ok(List(x))
        }
        (Op::Eq, x, y) => Ok(Bool(x == y)),
        (Op::Ne, x, y) => Ok(Bool(x != y)),
        (Op::Lt | Op::Le | Op::Gt | Op::Ge, Num(x), Num(y)) => Ok(Bool(compare(op, x.cmp(&y)))),
        (Op::Lt | Op::Le | Op::Gt | Op::Ge, Str(x), Str(y)) => Ok(Bool(compare(op, x.cmp(&y)))),
        (Op::In, x, List(items)) => Ok(Bool(items.contains(&x))),
        (Op::In, Str(x), Map(map)) => Ok(Bool(map.contains_key(&x))),
        (Op::In, Str(x), Str(y)) => Ok(Bool(y.contains(&x))),
        (op, x, y) => Err(Error::at(format!("cannot use `{}` on a {} and a {}", symbol(op), x.type_name(), y.type_name()), at)),
    }
}

fn symbol(op: Op) -> &'static str {
    match op {
        Op::Add => "+",
        Op::Sub => "-",
        Op::Mul => "*",
        Op::Div => "/",
        Op::Rem => "%",
        Op::Eq => "==",
        Op::Ne => "!=",
        Op::Lt => "<",
        Op::Le => "<=",
        Op::Gt => ">",
        Op::Ge => ">=",
        Op::In => "in",
    }
}

fn compare(op: Op, order: std::cmp::Ordering) -> bool {
    use std::cmp::Ordering::*;
    match op {
        Op::Lt => order == Less,
        Op::Le => order != Greater,
        Op::Gt => order == Greater,
        _ => order != Less,
    }
}

pub(crate) fn num(value: &Value) -> Result<Decimal> {
    value.as_num().ok_or_else(|| Error::new(format!("expected a number, found a {}", value.type_name())))
}
