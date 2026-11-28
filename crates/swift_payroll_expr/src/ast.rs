use std::collections::BTreeSet;

use crate::arith::Precision;
use crate::error::Result;
use crate::eval::{eval, Ctx, Env};
use crate::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    In,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Node {
    Lit(Value),
    Ident(String, usize),
    List(Vec<Node>),
    Map(Vec<(Node, Node)>),
    Member(Box<Node>, String, usize),
    Index(Box<Node>, Box<Node>, usize),
    Call(String, Vec<Node>, usize),
    Not(Box<Node>, usize),
    Neg(Box<Node>, usize),
    /// `first op rest op rest ...`, left to right. One flat node however long the chain, so a formula
    /// that adds up thousands of terms neither nests deeply nor overflows the stack.
    Chain(Box<Node>, Vec<(Op, Node, usize)>),
    And(Vec<Node>),
    Or(Vec<Node>),
    Cond(Box<Node>, Box<Node>, Box<Node>),
}

/// A compiled expression. Reading it once and evaluating it many times is the point.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub(crate) root: Node,
    pub(crate) source: String,
}

impl Expr {
    /// The text it was read from.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Run it with [`Precision::Round`]. The same environment always gives the same answer.
    pub fn eval(&self, env: &dyn Env) -> Result<Value> {
        self.eval_with(env, Precision::Round)
    }

    /// Run it with the given [`Precision`]: whether a result the number type cannot hold exactly is
    /// rounded or an error.
    pub fn eval_with(&self, env: &dyn Env, precision: Precision) -> Result<Value> {
        let ctx = Ctx { env, precision };
        eval(&self.root, &ctx, 0).map(std::borrow::Cow::into_owned)
    }

    /// Every name the expression reads (not the functions it calls, nor the keys after a `.`).
    pub fn identifiers(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        collect_idents(&self.root, &mut out);
        out
    }

    /// The text arguments of every call to `function` whose first argument is a literal string:
    /// for `total("taxable")`, `literal_arguments("total")` gives `taxable`. Lets a caller see what an
    /// expression depends on before running it.
    pub fn literal_arguments(&self, function: &str) -> Vec<String> {
        let mut out = Vec::new();
        collect_calls(&self.root, function, &mut out);
        out
    }

    /// The names given directly as argument `position` (from 0) of calls to `function`: for
    /// `bracket(pay, paye_table)`, `identifier_arguments("bracket", 1)` gives `paye_table`. Lets a host check
    /// a table before the expression ever runs.
    pub fn identifier_arguments(&self, function: &str, position: usize) -> Vec<String> {
        let mut out = Vec::new();
        walk(&self.root, &mut |node| {
            if let Node::Call(name, args, _) = node {
                if name == function {
                    if let Some(Node::Ident(argument, _)) = args.get(position) {
                        out.push(argument.clone());
                    }
                }
            }
        });
        out
    }

    /// Whether every call to `function` has the requested arity and a string literal at each listed argument
    /// position. A function with no calls satisfies this constraint.
    pub fn calls_match_literals(&self, function: &str, arity: usize, string_arguments: &[usize]) -> bool {
        let mut matches = true;
        walk(&self.root, &mut |node| {
            if let Node::Call(name, args, _) = node {
                if name == function && (args.len() != arity || string_arguments.iter().any(|&index| !matches!(args.get(index), Some(Node::Lit(Value::Str(_))))))
                {
                    matches = false;
                }
            }
        });
        matches
    }
}

fn walk(node: &Node, visit: &mut dyn FnMut(&Node)) {
    visit(node);
    match node {
        Node::Lit(_) | Node::Ident(..) => {}
        Node::List(items) => items.iter().for_each(|n| walk(n, visit)),
        Node::Map(entries) => entries.iter().for_each(|(k, v)| {
            walk(k, visit);
            walk(v, visit);
        }),
        Node::Member(inner, ..) | Node::Not(inner, _) | Node::Neg(inner, _) => walk(inner, visit),
        Node::Index(a, b, _) => {
            walk(a, visit);
            walk(b, visit);
        }
        Node::Chain(first, rest) => {
            walk(first, visit);
            rest.iter().for_each(|(_, n, _)| walk(n, visit));
        }
        Node::And(items) | Node::Or(items) => items.iter().for_each(|n| walk(n, visit)),
        Node::Call(_, args, _) => args.iter().for_each(|n| walk(n, visit)),
        Node::Cond(c, a, b) => {
            walk(c, visit);
            walk(a, visit);
            walk(b, visit);
        }
    }
}

fn collect_idents(node: &Node, out: &mut BTreeSet<String>) {
    walk(node, &mut |n| {
        if let Node::Ident(name, _) = n {
            out.insert(name.clone());
        }
    });
}

fn collect_calls(node: &Node, function: &str, out: &mut Vec<String>) {
    walk(node, &mut |n| {
        if let Node::Call(name, args, _) = n {
            if name == function {
                if let Some(Node::Lit(Value::Str(text))) = args.first() {
                    out.push(text.clone());
                }
            }
        }
    });
}
