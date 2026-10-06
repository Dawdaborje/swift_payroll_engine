use std::collections::BTreeSet;

use crate::error::Result;
use crate::eval::{eval, Env};
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
    Bin(Op, Box<Node>, Box<Node>, usize),
    And(Box<Node>, Box<Node>),
    Or(Box<Node>, Box<Node>),
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

    /// Run it. The same environment always gives the same answer.
    pub fn eval(&self, env: &dyn Env) -> Result<Value> {
        eval(&self.root, env, 0)
    }

    /// Every name the expression reads (not the functions it calls, nor the keys after a `.`).
    pub fn identifiers(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        collect_idents(&self.root, &mut out);
        out
    }

    /// The text arguments of every call to `function` whose first argument is a literal string:
    /// `sum("taxable")` gives `taxable`. Lets a caller see what an expression depends on before running it.
    pub fn literal_arguments(&self, function: &str) -> Vec<String> {
        let mut out = Vec::new();
        collect_calls(&self.root, function, &mut out);
        out
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
        Node::Index(a, b, _) | Node::Bin(_, a, b, _) | Node::And(a, b) | Node::Or(a, b) => {
            walk(a, visit);
            walk(b, visit);
        }
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
