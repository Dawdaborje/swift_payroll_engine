use rust_decimal::Decimal;

use crate::ast::{Expr, Node, Op};
use crate::error::{Error, Result};
use crate::value::Value;

const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(Decimal),
    Str(String),
    Ident(String),
    Punct(&'static str),
    End,
}

const PUNCT: &[&str] = &["&&", "||", "==", "!=", "<=", ">=", "+", "-", "*", "/", "%", "<", ">", "!", "?", ":", "(", ")", "[", "]", "{", "}", ",", "."];

fn lex(source: &str) -> Result<Vec<(Tok, usize)>> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_whitespace() {
            i += 1;
        } else if source[i..].starts_with("//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if c.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            // A dot makes a fraction only when a digit follows it (`3.5`); otherwise it is a member access.
            if i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let number = Decimal::from_str_exact(&source[start..i]).map_err(|_| Error::at(format!("`{}` is not a number this language can hold exactly", &source[start..i]), start))?;
            out.push((Tok::Num(number), start));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < bytes.len() && ((bytes[i] as char).is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            out.push((Tok::Ident(source[start..i].to_string()), start));
        } else if c == '"' || c == '\'' {
            let start = i;
            i += 1;
            let mut text = String::new();
            loop {
                let Some(&b) = bytes.get(i) else { return Err(Error::at("a string is not closed", start)) };
                i += 1;
                if b as char == c {
                    break;
                }
                if b == b'\\' {
                    let Some(&escaped) = bytes.get(i) else { return Err(Error::at("a string is not closed", start)) };
                    i += 1;
                    text.push(match escaped {
                        b'n' => '\n',
                        b't' => '\t',
                        b'\\' => '\\',
                        b'"' => '"',
                        b'\'' => '\'',
                        _ => return Err(Error::at("an unknown escape in a string", i - 1)),
                    });
                } else {
                    // Copy whole characters, not bytes.
                    let ch_start = i - 1;
                    let ch = source[ch_start..].chars().next().ok_or_else(|| Error::at("a string is not closed", start))?;
                    text.push(ch);
                    i = ch_start + ch.len_utf8();
                }
            }
            out.push((Tok::Str(text), start));
        } else {
            let Some(p) = PUNCT.iter().find(|p| source[i..].starts_with(**p)) else { return Err(Error::at(format!("unexpected `{c}`"), i)) };
            out.push((Tok::Punct(p), i));
            i += p.len();
        }
    }
    out.push((Tok::End, source.len()));
    Ok(out)
}

struct Parser {
    toks: Vec<(Tok, usize)>,
    pos: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].0
    }

    fn at(&self) -> usize {
        self.toks[self.pos].1
    }

    fn next(&mut self) -> Tok {
        let tok = self.toks[self.pos].0.clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        tok
    }

    fn eat(&mut self, p: &str) -> bool {
        if matches!(self.peek(), Tok::Punct(q) if *q == p) {
            self.next();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, p: &str) -> Result<()> {
        if self.eat(p) { Ok(()) } else { Err(Error::at(format!("expected `{p}`"), self.at())) }
    }

    fn enter(&mut self) -> Result<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(Error::at("the expression is nested too deeply", self.at()));
        }
        Ok(())
    }

    fn expr(&mut self) -> Result<Node> {
        self.enter()?;
        let cond = self.or()?;
        let node = if self.eat("?") {
            let yes = self.expr()?;
            self.expect(":")?;
            let no = self.expr()?;
            Node::Cond(Box::new(cond), Box::new(yes), Box::new(no))
        } else {
            cond
        };
        self.depth -= 1;
        Ok(node)
    }

    fn or(&mut self) -> Result<Node> {
        let mut left = self.and()?;
        while self.eat("||") {
            left = Node::Or(Box::new(left), Box::new(self.and()?));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Node> {
        let mut left = self.relation()?;
        while self.eat("&&") {
            left = Node::And(Box::new(left), Box::new(self.relation()?));
        }
        Ok(left)
    }

    fn relation(&mut self) -> Result<Node> {
        let mut left = self.additive()?;
        loop {
            let at = self.at();
            let op = match self.peek() {
                Tok::Punct("==") => Op::Eq,
                Tok::Punct("!=") => Op::Ne,
                Tok::Punct("<") => Op::Lt,
                Tok::Punct("<=") => Op::Le,
                Tok::Punct(">") => Op::Gt,
                Tok::Punct(">=") => Op::Ge,
                Tok::Ident(word) if word == "in" => Op::In,
                _ => return Ok(left),
            };
            self.next();
            left = Node::Bin(op, Box::new(left), Box::new(self.additive()?), at);
        }
    }

    fn additive(&mut self) -> Result<Node> {
        let mut left = self.multiplicative()?;
        loop {
            let at = self.at();
            let op = match self.peek() {
                Tok::Punct("+") => Op::Add,
                Tok::Punct("-") => Op::Sub,
                _ => return Ok(left),
            };
            self.next();
            left = Node::Bin(op, Box::new(left), Box::new(self.multiplicative()?), at);
        }
    }

    fn multiplicative(&mut self) -> Result<Node> {
        let mut left = self.unary()?;
        loop {
            let at = self.at();
            let op = match self.peek() {
                Tok::Punct("*") => Op::Mul,
                Tok::Punct("/") => Op::Div,
                Tok::Punct("%") => Op::Rem,
                _ => return Ok(left),
            };
            self.next();
            left = Node::Bin(op, Box::new(left), Box::new(self.unary()?), at);
        }
    }

    fn unary(&mut self) -> Result<Node> {
        let at = self.at();
        self.enter()?;
        let node = if self.eat("!") {
            Node::Not(Box::new(self.unary()?), at)
        } else if self.eat("-") {
            Node::Neg(Box::new(self.unary()?), at)
        } else {
            self.postfix()?
        };
        self.depth -= 1;
        Ok(node)
    }

    fn postfix(&mut self) -> Result<Node> {
        let mut node = self.primary()?;
        loop {
            let at = self.at();
            if self.eat(".") {
                match self.next() {
                    Tok::Ident(name) => node = Node::Member(Box::new(node), name, at),
                    _ => return Err(Error::at("a name after `.`", at)),
                }
            } else if self.eat("[") {
                let index = self.expr()?;
                self.expect("]")?;
                node = Node::Index(Box::new(node), Box::new(index), at);
            } else {
                return Ok(node);
            }
        }
    }

    fn primary(&mut self) -> Result<Node> {
        let at = self.at();
        match self.next() {
            Tok::Num(n) => Ok(Node::Lit(Value::Num(n))),
            Tok::Str(s) => Ok(Node::Lit(Value::Str(s))),
            Tok::Ident(name) => match name.as_str() {
                "true" => Ok(Node::Lit(Value::Bool(true))),
                "false" => Ok(Node::Lit(Value::Bool(false))),
                "null" => Ok(Node::Lit(Value::Null)),
                _ if self.eat("(") => {
                    let mut args = Vec::new();
                    if !self.eat(")") {
                        loop {
                            args.push(self.expr()?);
                            if self.eat(")") {
                                break;
                            }
                            self.expect(",")?;
                        }
                    }
                    Ok(Node::Call(name, args, at))
                }
                _ => Ok(Node::Ident(name, at)),
            },
            Tok::Punct("(") => {
                let inner = self.expr()?;
                self.expect(")")?;
                Ok(inner)
            }
            Tok::Punct("[") => {
                let mut items = Vec::new();
                if !self.eat("]") {
                    loop {
                        items.push(self.expr()?);
                        if self.eat("]") {
                            break;
                        }
                        self.expect(",")?;
                    }
                }
                Ok(Node::List(items))
            }
            Tok::Punct("{") => {
                let mut entries = Vec::new();
                if !self.eat("}") {
                    loop {
                        let key = self.expr()?;
                        self.expect(":")?;
                        entries.push((key, self.expr()?));
                        if self.eat("}") {
                            break;
                        }
                        self.expect(",")?;
                    }
                }
                Ok(Node::Map(entries))
            }
            Tok::End => Err(Error::at("the expression ends too soon", at)),
            other => Err(Error::at(format!("unexpected {}", describe(&other)), at)),
        }
    }
}

fn describe(tok: &Tok) -> String {
    match tok {
        Tok::Punct(p) => format!("`{p}`"),
        Tok::Num(n) => format!("`{n}`"),
        Tok::Str(s) => format!("{s:?}"),
        Tok::Ident(n) => format!("`{n}`"),
        Tok::End => "the end".to_string(),
    }
}

pub(crate) fn parse(source: &str) -> Result<Expr> {
    let mut parser = Parser { toks: lex(source)?, pos: 0, depth: 0 };
    let root = parser.expr()?;
    if !matches!(parser.peek(), Tok::End) {
        return Err(Error::at(format!("unexpected {}", describe(parser.peek())), parser.at()));
    }
    Ok(Expr { root, source: source.to_string() })
}
