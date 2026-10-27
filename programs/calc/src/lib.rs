//! `calc`: integer calculator.
//!
//! `calc 2 + 3 * 4` evaluates one expression; `calc` alone reads
//! expressions line by line until `exit` or Ctrl+D.
//!
//! Supports `+ - * / %`, parentheses and unary minus over 64-bit integers.
//! Division truncates toward zero; overflow is an error, not a wraparound.

#![no_std]

extern crate alloc;

use alloc::string::String;
use core::fmt;
use libpafos::io::{self, LineEnd};
use libpafos::{Exit, print, println};

const PROMPT: &str = "calc> ";
const MAX_LINE: usize = 80 - PROMPT.len() - 1;
/// Nesting limit for parentheses and unary signs, so a hostile input like
/// `((((...` cannot overflow the stack.
const MAX_DEPTH: usize = 64;

pub fn main(args: &[&str]) -> Exit {
    if args.is_empty() {
        return interactive();
    }
    let expr = args.join(" ");
    match eval(&expr) {
        Ok(value) => {
            println!("{}", value);
            Exit::Success
        }
        Err(err) => {
            println!("calc: {}", err);
            Exit::Error
        }
    }
}

fn interactive() -> Exit {
    println!("Integer calculator. `exit` or Ctrl+D to quit.");
    let mut line = String::new();
    loop {
        print!("{}", PROMPT);
        match io::read_line(&mut line, MAX_LINE) {
            LineEnd::Enter => {}
            LineEnd::Cancel => continue,
            LineEnd::Eof => return Exit::Success,
        }
        match line.trim() {
            "" => {}
            "exit" => return Exit::Success,
            expr => match eval(expr) {
                Ok(value) => println!("{}", value),
                Err(err) => println!("error: {}", err),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalcError {
    UnexpectedChar(char),
    UnexpectedEnd,
    DivisionByZero,
    Overflow,
    TooDeep,
}

impl fmt::Display for CalcError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CalcError::UnexpectedChar(c) => write!(f, "unexpected `{}`", c),
            CalcError::UnexpectedEnd => f.write_str("unexpected end of expression"),
            CalcError::DivisionByZero => f.write_str("division by zero"),
            CalcError::Overflow => f.write_str("number too large"),
            CalcError::TooDeep => f.write_str("expression nested too deeply"),
        }
    }
}

pub fn eval(expr: &str) -> Result<i64, CalcError> {
    let mut parser = Parser { input: expr.as_bytes(), pos: 0, depth: 0 };
    let value = parser.expr()?;
    match parser.peek() {
        None => Ok(value),
        Some(c) => Err(CalcError::UnexpectedChar(c as char)),
    }
}

/// Recursive descent over the grammar
/// ```text
/// expr    = term (('+' | '-') term)*
/// term    = unary (('*' | '/' | '%') unary)*
/// unary   = ('-' | '+') unary | primary
/// primary = number | '(' expr ')'
/// ```
struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
    depth: usize,
}

impl Parser<'_> {
    /// Next non-space byte, without consuming it.
    fn peek(&mut self) -> Option<u8> {
        while self.input.get(self.pos) == Some(&b' ') {
            self.pos += 1;
        }
        self.input.get(self.pos).copied()
    }

    fn expr(&mut self) -> Result<i64, CalcError> {
        let mut value = self.term()?;
        while let Some(op @ (b'+' | b'-')) = self.peek() {
            self.pos += 1;
            let rhs = self.term()?;
            let result = if op == b'+' { value.checked_add(rhs) } else { value.checked_sub(rhs) };
            value = result.ok_or(CalcError::Overflow)?;
        }
        Ok(value)
    }

    fn term(&mut self) -> Result<i64, CalcError> {
        let mut value = self.unary()?;
        while let Some(op @ (b'*' | b'/' | b'%')) = self.peek() {
            self.pos += 1;
            let rhs = self.unary()?;
            value = match op {
                b'*' => value.checked_mul(rhs).ok_or(CalcError::Overflow)?,
                _ if rhs == 0 => return Err(CalcError::DivisionByZero),
                // i64::MIN / -1 is the one case that overflows.
                b'/' => value.checked_div(rhs).ok_or(CalcError::Overflow)?,
                _ => value.checked_rem(rhs).ok_or(CalcError::Overflow)?,
            };
        }
        Ok(value)
    }

    fn unary(&mut self) -> Result<i64, CalcError> {
        match self.peek() {
            Some(op @ (b'-' | b'+')) => {
                self.pos += 1;
                let value = self.nested(Self::unary)?;
                if op == b'-' { value.checked_neg().ok_or(CalcError::Overflow) } else { Ok(value) }
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<i64, CalcError> {
        match self.peek() {
            Some(b'(') => {
                self.pos += 1;
                let value = self.nested(Self::expr)?;
                match self.peek() {
                    Some(b')') => {
                        self.pos += 1;
                        Ok(value)
                    }
                    Some(c) => Err(CalcError::UnexpectedChar(c as char)),
                    None => Err(CalcError::UnexpectedEnd),
                }
            }
            Some(c) if c.is_ascii_digit() => self.number(),
            Some(c) => Err(CalcError::UnexpectedChar(c as char)),
            None => Err(CalcError::UnexpectedEnd),
        }
    }

    fn number(&mut self) -> Result<i64, CalcError> {
        let mut value: i64 = 0;
        while let Some(&c) = self.input.get(self.pos).filter(|c| c.is_ascii_digit()) {
            value = value
                .checked_mul(10)
                .and_then(|v| v.checked_add((c - b'0') as i64))
                .ok_or(CalcError::Overflow)?;
            self.pos += 1;
        }
        Ok(value)
    }

    fn nested(&mut self, f: fn(&mut Self) -> Result<i64, CalcError>) -> Result<i64, CalcError> {
        if self.depth == MAX_DEPTH {
            return Err(CalcError::TooDeep);
        }
        self.depth += 1;
        let result = f(self);
        self.depth -= 1;
        result
    }
}
