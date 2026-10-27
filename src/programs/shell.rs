use crate::sys::{self, Exit, print, println};
use alloc::string::String;
use alloc::vec::Vec;

const PROMPT: &str = "pafos> ";
// Keeps prompt + input on a single 80-column VGA line, since backspace
// cannot move back across line boundaries.
const MAX_LINE: usize = 80 - PROMPT.len() - 1;

enum LineEnd {
    Enter,
    Cancel,
    Exit,
}

pub fn main(_args: &[&str]) -> Exit {
    println!("PAFOS shell. Type `help` for commands, Ctrl+D to exit.");
    let mut line = String::new();

    loop {
        print!("{}", PROMPT);
        match read_line(&mut line) {
            LineEnd::Enter => {}
            LineEnd::Cancel => continue,
            LineEnd::Exit => return Exit::Success,
        }

        let args: Vec<&str> = line.split_whitespace().collect();
        let Some((&command, rest)) = args.split_first() else {
            continue;
        };

        match command {
            "help" => help(),
            "echo" => println!("{}", rest.join(" ")),
            "clear" => sys::clear_screen(),
            "exit" => return Exit::Success,
            _ => println!("unknown command: {}", command),
        }
    }
}

fn read_line(line: &mut String) -> LineEnd {
    line.clear();
    loop {
        match sys::read_char() {
            '\n' => {
                println!();
                return LineEnd::Enter;
            }
            sys::CTRL_D => {
                println!();
                return LineEnd::Exit;
            }
            sys::CTRL_C => {
                println!("^C");
                sys::clear_stop();
                return LineEnd::Cancel;
            }
            sys::BACKSPACE => {
                if line.pop().is_some() {
                    sys::backspace();
                }
            }
            c if c.is_ascii_graphic() || c == ' ' => {
                if line.len() < MAX_LINE {
                    line.push(c);
                    print!("{}", c);
                }
            }
            _ => {}
        }
    }
}

fn help() {
    println!("help         show this message");
    println!("echo <text>  print text");
    println!("clear        clear the screen");
    println!("exit         exit the shell (same as Ctrl+D)");
    println!("Ctrl+C cancels the current line.");
}
