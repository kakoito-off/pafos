//! `calc`: integer calculator.
//!
//! `calc 2 + 3 * 4` evaluates one expression; `calc` alone reads
//! expressions line by line until `exit` or Ctrl+D.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use libpafos::io::{self, LineEnd};
use libpafos::{Exit, print, println};
use pafos_calc_eval::eval;

const PROMPT: &str = "calc> ";
const MAX_LINE: usize = 80 - PROMPT.len() - 1;

libpafos::entry!(main);

fn main(args: &[&str]) -> Exit {
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
