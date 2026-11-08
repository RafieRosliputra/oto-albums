//! Tiny arithmetic evaluator for lab formulas such as
//! `pages * paper_caliper_mm / 2 + 2.0`. Supports + - * / parentheses, numbers and variables.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ExprError {
    #[error("unexpected character '{0}' in formula")]
    BadChar(char),
    #[error("unknown variable '{0}' in formula")]
    UnknownVar(String),
    #[error("formula is incomplete or malformed")]
    Malformed,
    #[error("division by zero in formula")]
    DivZero,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(char),
}

fn lex(s: &str) -> Result<Vec<Tok>, ExprError> {
    let mut out = Vec::new();
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let st = i;
            while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.') {
                i += 1;
            }
            let t: String = cs[st..i].iter().collect();
            out.push(Tok::Num(t.parse().map_err(|_| ExprError::Malformed)?));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let st = i;
            while i < cs.len() && (cs[i].is_ascii_alphanumeric() || cs[i] == '_') {
                i += 1;
            }
            out.push(Tok::Ident(cs[st..i].iter().collect()));
        } else if "+-*/()".contains(c) {
            out.push(Tok::Op(c));
            i += 1;
        } else {
            return Err(ExprError::BadChar(c));
        }
    }
    Ok(out)
}

struct P<'a> {
    t: Vec<Tok>,
    i: usize,
    vars: &'a HashMap<&'a str, f64>,
}

impl P<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn expr(&mut self) -> Result<f64, ExprError> {
        let mut v = self.term()?;
        while let Some(Tok::Op(c @ ('+' | '-'))) = self.peek().cloned() {
            self.i += 1;
            let r = self.term()?;
            v = if c == '+' { v + r } else { v - r };
        }
        Ok(v)
    }
    fn term(&mut self) -> Result<f64, ExprError> {
        let mut v = self.factor()?;
        while let Some(Tok::Op(c @ ('*' | '/'))) = self.peek().cloned() {
            self.i += 1;
            let r = self.factor()?;
            if c == '/' {
                if r == 0.0 {
                    return Err(ExprError::DivZero);
                }
                v /= r;
            } else {
                v *= r;
            }
        }
        Ok(v)
    }
    fn factor(&mut self) -> Result<f64, ExprError> {
        match self.peek().cloned() {
            Some(Tok::Num(n)) => {
                self.i += 1;
                Ok(n)
            }
            Some(Tok::Ident(name)) => {
                self.i += 1;
                self.vars.get(name.as_str()).copied().ok_or(ExprError::UnknownVar(name))
            }
            Some(Tok::Op('-')) => {
                self.i += 1;
                Ok(-self.factor()?)
            }
            Some(Tok::Op('(')) => {
                self.i += 1;
                let v = self.expr()?;
                if self.peek() != Some(&Tok::Op(')')) {
                    return Err(ExprError::Malformed);
                }
                self.i += 1;
                Ok(v)
            }
            _ => Err(ExprError::Malformed),
        }
    }
}

pub fn eval(s: &str, vars: &HashMap<&str, f64>) -> Result<f64, ExprError> {
    let t = lex(s)?;
    let mut p = P { t, i: 0, vars };
    let v = p.expr()?;
    if p.i != p.t.len() {
        return Err(ExprError::Malformed);
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spine_formula() {
        let mut vars = HashMap::new();
        vars.insert("pages", 40.0);
        vars.insert("paper_caliper_mm", 0.5);
        let v = eval("pages * paper_caliper_mm / 2 + 2.0", &vars).unwrap();
        assert!((v - 12.0).abs() < 1e-9);
        assert!(eval("(1+2)*3", &vars).unwrap() == 9.0);
        assert!(matches!(eval("pages *", &vars), Err(ExprError::Malformed)));
        assert!(matches!(eval("foo + 1", &vars), Err(ExprError::UnknownVar(_))));
        assert!(matches!(eval("1/0", &vars), Err(ExprError::DivZero)));
    }
}
