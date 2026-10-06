use std::collections::BTreeMap;

use rust_decimal::Decimal;

use crate::ast::{Node, Op};
use crate::builtins;
use crate::error::{Error, Result};
use crate::value::Value;

const MAX_DEPTH: usize = 128;

/// What an expression can see: names, and functions the host adds to the built-in ones.
pub trait Env {
    fn get(&self, name: &str) -> Option<Value>;

    /// A host function. `None` means "not mine": the built-ins are tried next.
    fn call(&self, _name: &str, _args: &[Value]) -> Option<Result<Value>> {
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

pub(crate) fn eval(node: &Node, env: &dyn Env, depth: usize) -> Result<Value> {
    if depth > MAX_DEPTH {
        return Err(Error::new("the expression is nested too deeply"));
    }
    let next = depth + 1;
    match node {
        Node::Lit(value) => Ok(value.clone()),
        Node::Ident(name, at) => env.get(name).ok_or_else(|| Error::at(format!("`{name}` is not defined"), *at)),
        Node::List(items) => Ok(Value::List(items.iter().map(|n| eval(n, env, next)).collect::<Result<_>>()?)),
        Node::Map(entries) => {
            let mut map = BTreeMap::new();
            for (key, value) in entries {
                match eval(key, env, next)? {
                    Value::Str(key) => {
                        map.insert(key, eval(value, env, next)?);
                    }
                    other => return Err(Error::new(format!("a map key is text, not a {}", other.type_name()))),
                }
            }
            Ok(Value::Map(map))
        }
        Node::Member(inner, key, at) => match eval(inner, env, next)? {
            Value::Map(map) => map.get(key).cloned().ok_or_else(|| Error::at(format!("there is no `{key}`"), *at)),
            other => Err(Error::at(format!("`.{key}` needs a map, not a {}", other.type_name()), *at)),
        },
        Node::Index(inner, index, at) => match (eval(inner, env, next)?, eval(index, env, next)?) {
            (Value::List(items), Value::Num(n)) => {
                let i = usize::try_from(i64::try_from(n.trunc()).map_err(|_| Error::at("a list position is a whole number", *at))?).map_err(|_| Error::at("a list position is not negative", *at))?;
                items.get(i).cloned().ok_or_else(|| Error::at(format!("there is no item {i}"), *at))
            }
            (Value::Map(map), Value::Str(key)) => map.get(&key).cloned().ok_or_else(|| Error::at(format!("there is no `{key}`"), *at)),
            (a, b) => Err(Error::at(format!("cannot index a {} by a {}", a.type_name(), b.type_name()), *at)),
        },
        Node::Call(name, args, at) => {
            let values = args.iter().map(|n| eval(n, env, next)).collect::<Result<Vec<_>>>()?;
            let result = match env.call(name, &values) {
                Some(result) => result,
                None => builtins::call(name, &values),
            };
            result.map_err(|e| Error { message: format!("{name}: {}", e.message), at: e.at.or(Some(*at)) })
        }
        Node::Not(inner, at) => Ok(Value::Bool(!truth(&eval(inner, env, next)?, Some(*at))?)),
        Node::Neg(inner, at) => match eval(inner, env, next)? {
            Value::Num(n) => Ok(Value::Num(-n)),
            other => Err(Error::at(format!("cannot negate a {}", other.type_name()), *at)),
        },
        Node::And(a, b) => {
            if !truth(&eval(a, env, next)?, None)? {
                return Ok(Value::Bool(false));
            }
            Ok(Value::Bool(truth(&eval(b, env, next)?, None)?))
        }
        Node::Or(a, b) => {
            if truth(&eval(a, env, next)?, None)? {
                return Ok(Value::Bool(true));
            }
            Ok(Value::Bool(truth(&eval(b, env, next)?, None)?))
        }
        Node::Cond(c, yes, no) => {
            if truth(&eval(c, env, next)?, None)? {
                eval(yes, env, next)
            } else {
                eval(no, env, next)
            }
        }
        Node::Bin(op, a, b, at) => binary(*op, eval(a, env, next)?, eval(b, env, next)?, *at),
    }
}

fn binary(op: Op, a: Value, b: Value, at: usize) -> Result<Value> {
    use Value::*;
    match (op, a, b) {
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
        (op, x, y) => Err(Error::at(format!("cannot use {op:?} on a {} and a {}", x.type_name(), y.type_name()), at)),
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
