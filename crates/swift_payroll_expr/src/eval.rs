use std::borrow::Cow;
use std::collections::BTreeMap;

use rust_decimal::Decimal;

use crate::arith::{self, Precision};
use crate::ast::{Node, Op};
use crate::builtins;
use crate::error::{Error, ErrorKind, Result};
use crate::value::Value;

/// A backstop: the parser already bounds how deep a tree can be (see `parse.rs`), well below this.
const MAX_DEPTH: usize = 2048;

/// What an expression can see: names, and functions the host adds to the built-in ones.
pub trait Env {
    /// The value of a name, or `None` when the environment has no such name (an error, never zero).
    fn get(&self, name: &str) -> Option<Value>;

    /// Like [`get`](Env::get), but a host that keeps its values can lend them instead of copying: a table
    /// of bands read by every formula for every person is then never cloned. The default copies.
    fn get_ref(&self, name: &str) -> Option<Cow<'_, Value>> {
        self.get(name).map(Cow::Owned)
    }

    /// A host function. `None` means "not mine": the built-ins are tried next. A host function with the
    /// name of a built-in replaces it.
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

/// One evaluation: the environment and how arithmetic treats inexact results.
pub(crate) struct Ctx<'a> {
    pub(crate) env: &'a dyn Env,
    pub(crate) precision: Precision,
}

fn truth(value: &Value, at: Option<usize>) -> Result<bool> {
    match value {
        Value::Bool(b) => Ok(*b),
        other => Err(Error { kind: ErrorKind::Type, message: format!("a condition is true or false, not a {}", other.type_name()), at }),
    }
}

/// Give an error without a position the position of the node it came from.
fn placed(error: Error, at: usize) -> Error {
    Error { at: error.at.or(Some(at)), ..error }
}

/// Evaluate to a value that is borrowed when it already exists somewhere (a literal in the expression, a
/// name in the environment, a member of a map) and built only when something was calculated.
pub(crate) fn eval<'a>(node: &'a Node, ctx: &Ctx<'a>, depth: usize) -> Result<Cow<'a, Value>> {
    if depth > MAX_DEPTH {
        return Err(Error::new(ErrorKind::TooDeep, "the expression is nested too deeply"));
    }
    let next = depth + 1;
    match node {
        Node::Lit(value) => Ok(Cow::Borrowed(value)),
        Node::Ident(name, at) => ctx.env.get_ref(name).ok_or_else(|| Error::at(ErrorKind::Undefined, format!("`{name}` is not defined"), *at)),
        Node::List(items) => Ok(Cow::Owned(Value::List(items.iter().map(|n| eval(n, ctx, next).map(Cow::into_owned)).collect::<Result<_>>()?))),
        Node::Map(entries) => {
            let mut map = BTreeMap::new();
            for (key, value) in entries {
                match eval(key, ctx, next)?.into_owned() {
                    Value::Str(key) => {
                        map.insert(key, eval(value, ctx, next)?.into_owned());
                    }
                    other => return Err(Error::kind_of_type(format!("a map key is text, not a {}", other.type_name()))),
                }
            }
            Ok(Cow::Owned(Value::Map(map)))
        }
        Node::Member(inner, key, at) => match eval(inner, ctx, next)? {
            Cow::Borrowed(Value::Map(map)) => {
                map.get(key).map(Cow::Borrowed).ok_or_else(|| Error::at(ErrorKind::Undefined, format!("there is no `{key}`"), *at))
            }
            Cow::Owned(Value::Map(mut map)) => {
                map.remove(key).map(Cow::Owned).ok_or_else(|| Error::at(ErrorKind::Undefined, format!("there is no `{key}`"), *at))
            }
            other => Err(Error::at(ErrorKind::Type, format!("`.{key}` needs a map, not a {}", other.type_name()), *at)),
        },
        Node::Index(inner, index, at) => {
            let container = eval(inner, ctx, next)?;
            let index = eval(index, ctx, next)?;
            match (&*container, &*index) {
                (Value::List(items), Value::Num(n)) => {
                    if !n.fract().is_zero() {
                        return Err(Error::at(ErrorKind::InvalidArgument, "a list position is a whole number", *at));
                    }
                    if n.is_sign_negative() && !n.is_zero() {
                        return Err(Error::at(ErrorKind::InvalidArgument, "a list position is not negative", *at));
                    }
                    // A position too large for this machine's usize (above 2^32 in WebAssembly) is simply not in the list.
                    let found = u64::try_from(*n)
                        .ok()
                        .and_then(|i| usize::try_from(i).ok())
                        .and_then(|i| items.get(i))
                        .ok_or_else(|| Error::at(ErrorKind::Undefined, format!("there is no item {}", n.normalize()), *at))?;
                    Ok(Cow::Owned(found.clone()))
                }
                (Value::Map(map), Value::Str(key)) => {
                    let found = map.get(key).ok_or_else(|| Error::at(ErrorKind::Undefined, format!("there is no `{key}`"), *at))?;
                    Ok(Cow::Owned(found.clone()))
                }
                (a, b) => Err(Error::at(ErrorKind::Type, format!("cannot index a {} by a {}", a.type_name(), b.type_name()), *at)),
            }
        }
        Node::Call(name, args, at) => {
            // Most calls have a few arguments: keep them on the stack.
            let mut stack: [Cow<'a, Value>; 4] = [Cow::Borrowed(&NULL), Cow::Borrowed(&NULL), Cow::Borrowed(&NULL), Cow::Borrowed(&NULL)];
            let mut heap: Vec<Cow<'a, Value>> = Vec::new();
            let values: &[Cow<'a, Value>] = if args.len() <= stack.len() {
                for (slot, arg) in stack.iter_mut().zip(args) {
                    *slot = eval(arg, ctx, next)?;
                }
                &stack[..args.len()]
            } else {
                for arg in args {
                    heap.push(eval(arg, ctx, next)?);
                }
                &heap
            };
            let mut small: [&Value; 4] = [&NULL; 4];
            let large: Vec<&Value>;
            let refs: &[&Value] = if values.len() <= small.len() {
                for (slot, value) in small.iter_mut().zip(values) {
                    *slot = value;
                }
                &small[..values.len()]
            } else {
                large = values.iter().map(|v| &**v).collect();
                &large
            };
            let result = match ctx.env.call(name, refs) {
                Some(result) => result,
                None => builtins::call(name, refs, ctx.precision),
            };
            result.map(Cow::Owned).map_err(|e| Error { message: format!("{name}: {}", e.message), at: e.at.or(Some(*at)), kind: e.kind })
        }
        Node::Not(inner, at) => Ok(Cow::Owned(Value::Bool(!truth(&*eval(inner, ctx, next)?, Some(*at))?))),
        Node::Neg(inner, at) => match &*eval(inner, ctx, next)? {
            // `-0` stays `0`: a negative zero would show as `-0` in text.
            Value::Num(n) => Ok(Cow::Owned(Value::Num(if n.is_zero() { *n } else { -*n }))),
            other => Err(Error::at(ErrorKind::Type, format!("cannot negate a {}", other.type_name()), *at)),
        },
        Node::And(items) => {
            for item in items {
                if !truth(&*eval(item, ctx, next)?, None)? {
                    return Ok(Cow::Owned(Value::Bool(false)));
                }
            }
            Ok(Cow::Owned(Value::Bool(true)))
        }
        Node::Or(items) => {
            for item in items {
                if truth(&*eval(item, ctx, next)?, None)? {
                    return Ok(Cow::Owned(Value::Bool(true)));
                }
            }
            Ok(Cow::Owned(Value::Bool(false)))
        }
        Node::Cond(c, yes, no) => {
            if truth(&*eval(c, ctx, next)?, None)? {
                eval(yes, ctx, next)
            } else {
                eval(no, ctx, next)
            }
        }
        Node::Chain(first, rest) => {
            let mut acc = eval(first, ctx, next)?;
            for (op, node, at) in rest {
                let right = eval(node, ctx, next)?;
                acc = Cow::Owned(binary(*op, acc, right, *at, ctx.precision)?);
            }
            Ok(acc)
        }
    }
}

static NULL: Value = Value::Null;

fn numbers(op: Op, x: Decimal, y: Decimal, precision: Precision) -> Result<Value> {
    use Value::{Bool, Num};
    match op {
        Op::Add => arith::add(x, y, precision).map(Num),
        Op::Sub => arith::sub(x, y, precision).map(Num),
        Op::Mul => arith::mul(x, y, precision).map(Num),
        Op::Div => arith::div(x, y, precision).map(Num),
        Op::Rem => arith::rem(x, y).map(Num),
        Op::Eq => Ok(Bool(x == y)),
        Op::Ne => Ok(Bool(x != y)),
        Op::Lt | Op::Le | Op::Gt | Op::Ge => Ok(Bool(compare(op, x.cmp(&y)))),
        Op::In => Err(Error::kind_of_type(format!("cannot use `{}` on a number and a number", symbol(op)))),
    }
}

fn binary(op: Op, a: Cow<'_, Value>, b: Cow<'_, Value>, at: usize, precision: Precision) -> Result<Value> {
    use Value::*;
    // Numbers are by far the commonest operands: handle them without ever copying anything.
    if let (Num(x), Num(y)) = (&*a, &*b) {
        return numbers(op, *x, *y, precision).map_err(|e| placed(e, at));
    }
    // `"3" == 3` is false in most languages; in payroll it silently drops a component paid by condition,
    // so comparing values of different kinds (other than with `null`) is an error, as `<` already is.
    let mismatched = |x: &Value, y: &Value| !matches!(x, Null) && !matches!(y, Null) && std::mem::discriminant(x) != std::mem::discriminant(y);
    let refuse =
        |x: &Value, y: &Value| Err(Error::at(ErrorKind::Type, format!("cannot use `{}` on a {} and a {}", symbol(op), x.type_name(), y.type_name()), at));
    match (op, &*a, &*b) {
        (Op::Eq | Op::Ne, x, y) if mismatched(x, y) => return refuse(x, y),
        (Op::In, x, List(items)) => {
            if let Some(item) = items.iter().find(|item| mismatched(x, item)) {
                return Err(Error::at(ErrorKind::Type, format!("cannot look for a {} in a list holding a {}", x.type_name(), item.type_name()), at));
            }
        }
        _ => {}
    }
    match (op, a.into_owned(), b.into_owned()) {
        (Op::Add, Str(x), Str(y)) => Ok(Str(x + &y)),
        (Op::Add, List(mut x), List(y)) => {
            x.extend(y);
            Ok(List(x))
        }
        (Op::Eq, x, y) => Ok(Bool(x == y)),
        (Op::Ne, x, y) => Ok(Bool(x != y)),
        (Op::Lt | Op::Le | Op::Gt | Op::Ge, Str(x), Str(y)) => Ok(Bool(compare(op, x.cmp(&y)))),
        (Op::In, x, List(items)) => Ok(Bool(items.contains(&x))),
        (Op::In, Str(x), Map(map)) => Ok(Bool(map.contains_key(&x))),
        (Op::In, Str(x), Str(y)) => Ok(Bool(y.contains(&x))),
        (op, x, y) => Err(Error::at(ErrorKind::Type, format!("cannot use `{}` on a {} and a {}", symbol(op), x.type_name(), y.type_name()), at)),
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
    value.as_num().ok_or_else(|| Error::kind_of_type(format!("expected a number, found a {}", value.type_name())))
}
