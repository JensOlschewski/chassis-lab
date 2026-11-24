//! The restricted MATLAB expression subset of optimization problems.
//!
//! Expressions are parsed only to be checked and rewritten. The solver
//! evaluates the rewritten text: names become `s.<name>` and the vector
//! components `.x/.y/.z` become `xyz(<expr>,<1|2|3>)`.

use std::collections::BTreeSet;

use anyhow::{Result, bail};

pub const FUNCTIONS: &[&str] = &[
    "abs", "sqrt", "atan", "atan2", "rad2deg", "deg2rad", "round", "numel", "max", "min", "mean",
    "gradient", "cross", "dot", "vecnorm", "interp1",
];
const CONSTANTS: &[&str] = &["pi"];
/// Names a model or signal must not take.
pub const RESERVED: &[&str] = &["x", "y", "z", "end", "s", "xyz"];

pub fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name.len() <= 63
}

pub fn is_reserved(name: &str) -> bool {
    RESERVED.contains(&name) || FUNCTIONS.contains(&name) || CONSTANTS.contains(&name)
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(String),
    Ident(String),
    Op(&'static str),
}

const OPERATORS: &[&str] = &[
    ".*", "./", ".^", "+", "-", "*", "/", "^", "(", ")", ",", ":", ".",
];

fn tokenize(text: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next_is_digit = chars.get(i + 1).is_some_and(char::is_ascii_digit);
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || (c == '.' && next_is_digit) {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                // `.*`, `./` and `.^` after a digit belong to the operator
                if chars[i] == '.' && matches!(chars.get(i + 1), Some('*' | '/' | '^')) {
                    break;
                }
                i += 1;
            }
            if matches!(chars.get(i), Some('e' | 'E')) {
                let sign = matches!(chars.get(i + 1), Some('+' | '-'));
                if chars
                    .get(i + 1 + usize::from(sign))
                    .is_some_and(char::is_ascii_digit)
                {
                    i += 2 + usize::from(sign);
                    while chars.get(i).is_some_and(char::is_ascii_digit) {
                        i += 1;
                    }
                }
            }
            let literal: String = chars[start..i].iter().collect();
            if literal.parse::<f64>().is_err() {
                bail!("`{literal}` is not a number");
            }
            tokens.push(Token::Number(literal));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            tokens.push(Token::Ident(chars[start..i].iter().collect()));
        } else {
            let rest: String = chars[i..].iter().take(2).collect();
            let Some(op) = OPERATORS.iter().find(|op| rest.starts_with(**op)) else {
                bail!("`{c}` is not allowed in an expression{}", hint(c));
            };
            tokens.push(Token::Op(op));
            i += op.len();
        }
    }
    Ok(tokens)
}

fn hint(c: char) -> &'static str {
    match c {
        '\'' | '"' => " (no strings)",
        '@' => " (no function handles)",
        ';' | '=' | '<' | '>' | '~' | '&' | '|' => " (no assignments or comparisons)",
        '[' | ']' | '{' | '}' => " (no arrays: address vectors by name and `.x/.y/.z`)",
        _ => "",
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(String),
    Name(String),
    End,
    Field(Box<Expr>, String),
    /// A whitelisted function call.
    Call(String, Vec<Expr>),
    /// Indexing a model or signal along the sample axis.
    Index(Box<Expr>, Vec<Expr>),
    Range(Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
    Binary(Box<Expr>, &'static str, Box<Expr>),
}

pub fn parse(text: &str) -> Result<Expr> {
    let mut parser = Parser {
        tokens: tokenize(text)?,
        at: 0,
        in_index: false,
    };
    let expr = parser.sum()?;
    if let Some(token) = parser.peek() {
        bail!("unexpected {}", describe(token));
    }
    Ok(expr)
}

fn describe(token: &Token) -> String {
    match token {
        Token::Number(n) => format!("`{n}`"),
        Token::Ident(n) => format!("`{n}`"),
        Token::Op(op) => format!("`{op}`"),
    }
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
    in_index: bool,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn eat(&mut self, op: &str) -> bool {
        if self.peek() == Some(&Token::Op(OPERATORS.iter().find(|o| **o == op).unwrap())) {
            self.at += 1;
            return true;
        }
        false
    }

    fn expect(&mut self, op: &str) -> Result<()> {
        if self.eat(op) {
            Ok(())
        } else {
            match self.peek() {
                Some(token) => bail!("expected `{op}`, found {}", describe(token)),
                None => bail!("expected `{op}`, found the end of the expression"),
            }
        }
    }

    fn sum(&mut self) -> Result<Expr> {
        let mut left = self.product()?;
        loop {
            let op = if self.eat("+") {
                "+"
            } else if self.eat("-") {
                "-"
            } else {
                return Ok(left);
            };
            left = Expr::Binary(left.into(), op, self.product()?.into());
        }
    }

    fn product(&mut self) -> Result<Expr> {
        let mut left = self.unary()?;
        loop {
            let op = [".*", "./", "*", "/"].into_iter().find(|op| self.eat(op));
            let Some(op) = op else { return Ok(left) };
            left = Expr::Binary(left.into(), op, self.unary()?.into());
        }
    }

    fn unary(&mut self) -> Result<Expr> {
        if self.eat("-") {
            return Ok(Expr::Neg(self.unary()?.into()));
        }
        if self.eat("+") {
            return self.unary();
        }
        let base = self.postfix()?;
        let op = [".^", "^"].into_iter().find(|op| self.eat(op));
        match op {
            Some(op) => Ok(Expr::Binary(base.into(), op, self.unary()?.into())),
            None => Ok(base),
        }
    }

    fn postfix(&mut self) -> Result<Expr> {
        let mut expr = self.primary()?;
        loop {
            if self.eat(".") {
                let Some(Token::Ident(name)) = self.peek().cloned() else {
                    bail!("expected a field name after `.`");
                };
                self.at += 1;
                expr = Expr::Field(expr.into(), name);
            } else if self.eat("(") {
                expr = self.arguments(expr)?;
            } else {
                return Ok(expr);
            }
        }
    }

    fn arguments(&mut self, target: Expr) -> Result<Expr> {
        let indexing = !matches!(&target, Expr::Name(name) if FUNCTIONS.contains(&name.as_str()));
        if !is_chain(&target) {
            bail!("only a model or signal can be indexed or called, not a result");
        }
        let outer = self.in_index;
        self.in_index = indexing || outer;
        let mut args = Vec::new();
        if !self.eat(")") {
            loop {
                if self.peek() == Some(&Token::Op(":")) {
                    bail!(
                        "`:` needs both ends (`a:b`). Select signals by name, not by row or column"
                    );
                }
                let mut arg = self.sum()?;
                if indexing && self.eat(":") {
                    arg = Expr::Range(arg.into(), self.sum()?.into());
                }
                args.push(arg);
                if self.eat(")") {
                    break;
                }
                self.expect(",")?;
            }
        }
        self.in_index = outer;
        match target {
            Expr::Name(name) if !indexing => Ok(Expr::Call(name, args)),
            target if args.len() == 1 => Ok(Expr::Index(target.into(), args)),
            _ => bail!(
                "only one index is allowed, along the sample axis. Address bodies, joints and points by name"
            ),
        }
    }

    fn primary(&mut self) -> Result<Expr> {
        match self.peek().cloned() {
            Some(Token::Number(n)) => {
                self.at += 1;
                Ok(Expr::Number(n))
            }
            Some(Token::Ident(name)) => {
                self.at += 1;
                if name == "end" {
                    if !self.in_index {
                        bail!("`end` is only allowed inside an index");
                    }
                    return Ok(Expr::End);
                }
                Ok(Expr::Name(name))
            }
            Some(Token::Op("(")) => {
                self.at += 1;
                let inner = self.sum()?;
                self.expect(")")?;
                Ok(inner)
            }
            Some(token) => bail!("unexpected {}", describe(&token)),
            None => bail!("the expression ends too early"),
        }
    }
}

/// A name followed by fields: the only thing MATLAB lets us index directly.
fn is_chain(expr: &Expr) -> bool {
    match expr {
        Expr::Name(_) => true,
        Expr::Field(inner, _) => is_chain(inner),
        _ => false,
    }
}

pub(crate) const COMPONENTS: [&str; 3] = ["x", "y", "z"];

fn component(name: &str) -> Option<usize> {
    COMPONENTS.iter().position(|c| *c == name)
}

impl Expr {
    /// Names used as models or signals (not functions or constants), in no particular order.
    pub fn references(&self) -> BTreeSet<&str> {
        let mut names = BTreeSet::new();
        self.visit(&mut |expr| {
            if let Expr::Name(name) = expr
                && !CONSTANTS.contains(&name.as_str())
            {
                names.insert(name.as_str());
            }
        });
        names
    }

    /// The root name and the field names of a chain such as `pwt.bodies.B1.points`.
    pub fn field_paths(&self) -> Vec<(&str, Vec<&str>)> {
        let mut paths = Vec::new();
        self.visit(&mut |expr| {
            let mut fields = Vec::new();
            let mut at = expr;
            while let Expr::Field(inner, name) = at {
                fields.push(name.as_str());
                at = inner;
            }
            if let (Expr::Name(root), false) = (at, fields.is_empty()) {
                fields.reverse();
                paths.push((root.as_str(), fields));
            }
        });
        paths
    }

    fn visit<'a>(&'a self, f: &mut impl FnMut(&'a Expr)) {
        f(self);
        match self {
            Expr::Field(inner, _) | Expr::Neg(inner) => inner.visit(f),
            Expr::Call(_, args) => args.iter().for_each(|a| a.visit(f)),
            Expr::Index(target, args) => {
                target.visit(f);
                args.iter().for_each(|a| a.visit(f));
            }
            Expr::Range(a, b) | Expr::Binary(a, _, b) => {
                a.visit(f);
                b.visit(f);
            }
            Expr::Number(_) | Expr::Name(_) | Expr::End => {}
        }
    }

    /// Checks that every referenced name is a known model or signal.
    pub fn check_names(&self, known: &impl Fn(&str) -> bool) -> Result<()> {
        for name in self.references() {
            if !known(name) {
                bail!("unknown name `{name}`");
            }
        }
        Ok(())
    }

    /// MATLAB text for the solver: names read from the struct `s`, components via `xyz`.
    pub fn rewrite(&self) -> String {
        match self {
            Expr::Number(n) => n.clone(),
            Expr::Name(name) if CONSTANTS.contains(&name.as_str()) => name.clone(),
            Expr::Name(name) => format!("s.{name}"),
            Expr::End => "end".to_owned(),
            Expr::Field(inner, name) => match component(name) {
                Some(axis) => format!("xyz({},{})", inner.rewrite(), axis + 1),
                None => format!("{}.{name}", inner.rewrite()),
            },
            Expr::Call(name, args) => format!("{name}({})", join(args)),
            // `.x(i)` indexes row x of the 3×n matrix directly: MATLAB cannot index a call
            Expr::Index(target, args) => match &**target {
                Expr::Field(inner, name) if component(name).is_some() => format!(
                    "{}({},{})",
                    inner.rewrite(),
                    component(name).unwrap() + 1,
                    join(args)
                ),
                target => format!("{}({})", target.rewrite(), join(args)),
            },
            Expr::Range(a, b) => format!("{}:{}", a.rewrite(), b.rewrite()),
            Expr::Neg(inner) => format!("(-{})", inner.rewrite()),
            Expr::Binary(a, op, b) => format!("({}{op}{})", a.rewrite(), b.rewrite()),
        }
    }
}

fn join(args: &[Expr]) -> String {
    args.iter().map(Expr::rewrite).collect::<Vec<_>>().join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rewrite(text: &str) -> String {
        parse(text).unwrap().rewrite()
    }

    fn error(text: &str) -> String {
        parse(text).unwrap_err().to_string()
    }

    #[test]
    fn rewrites_names_and_components() {
        assert_eq!(rewrite("v.x"), "xyz(s.v,1)");
        assert_eq!(rewrite("a.b.z(end)"), "s.a.b(3,end)");
        assert_eq!(
            rewrite("rad2deg(atan(v.x ./ v.z))"),
            "rad2deg(atan((xyz(s.v,1)./xyz(s.v,3))))"
        );
        assert_eq!(rewrite("cross(a.b.c, r) + v"), "(cross(s.a.b.c,s.r)+s.v)");
        assert_eq!(rewrite("x2(round(end/2))"), "s.x2(round((end/2)))");
        assert_eq!(rewrite("e(2:end)"), "s.e(2:end)");
        assert_eq!(rewrite("-a^2 + 1e-3*pi"), "((-(s.a^2))+(1e-3*pi))");
        assert_eq!(rewrite("2.*a"), "(2.*s.a)");
    }

    #[test]
    fn nested_component_suffixes_wrap_inner_first() {
        assert_eq!(rewrite("f.a.x"), "xyz(s.f.a,1)");
        assert_eq!(rewrite("(a - b).y"), "xyz((s.a-s.b),2)");
        assert_eq!(rewrite("abs(a.x(end))"), "abs(s.a(1,end))");
        assert_eq!(rewrite("a.x(round(end/2))"), "s.a(1,round((end/2)))");
    }

    #[test]
    fn rejects_unsupported_syntax() {
        assert!(error("'a'").contains("no strings"));
        assert!(error("@sin").contains("function handles"));
        assert!(error("a; b").contains("assignments"));
        assert!(error("a = 1").contains("assignments"));
        assert!(error("a < 1").contains("comparisons"));
        assert!(error("[1 2]").contains("no arrays"));
        assert!(error("a(3,:)").contains("`:` needs both ends"));
        assert!(error("a(3,4)").contains("only one index"));
        assert!(error("max(a)(1)").contains("not a result"));
        assert!(error("(a - b)(1)").contains("not a result"));
        assert!(error("end + 1").contains("only allowed inside an index"));
        assert!(error("a +").contains("ends too early"));
        assert!(error("(a").contains("expected `)`"));
        assert!(error("1.2.3").contains("not a number"));
    }

    #[test]
    fn collects_references_and_paths() {
        let expr = parse("max(abs(a.b.c)) + sig(end) * pi").unwrap();
        let names: Vec<_> = expr.references().into_iter().collect();
        assert_eq!(names, ["a", "sig"]);
        assert!(expr.field_paths().contains(&("a", vec!["b", "c"])));
    }
}
