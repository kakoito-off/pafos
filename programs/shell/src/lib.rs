#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use libpafos::fs::{self, File, Mode};
use libpafos::io::{self, LineEnd};
use libpafos::{Error, Exit, print, println, process};

const PROMPT: &str = "pafos> ";
// Keeps prompt + input on a single 80-column VGA line, since backspace
// cannot move back across line boundaries.
const MAX_LINE: usize = 80 - PROMPT.len() - 1;

pub fn main(_args: &[&str]) -> Exit {
    println!("PAFOS shell. Type `help` for commands, Ctrl+D to exit.");
    let mut line = String::new();

    loop {
        print!("{}", PROMPT);
        match io::read_line(&mut line, MAX_LINE) {
            LineEnd::Enter => {}
            LineEnd::Cancel => continue,
            LineEnd::Eof => return Exit::Success,
        }

        let args: Vec<&str> = line.split_whitespace().collect();
        let Some((&command, rest)) = args.split_first() else {
            continue;
        };

        let result = match command {
            "exit" => return Exit::Success,
            "help" => {
                help();
                Ok(())
            }
            "echo" => {
                println!("{}", rest.join(" "));
                Ok(())
            }
            "clear" => {
                io::clear_screen();
                Ok(())
            }
            "ls" => {
                ls();
                Ok(())
            }
            "df" => df(),
            "cat" => with_name(command, rest, cat),
            "create" => with_name(command, rest, fs::create),
            "rm" => with_name(command, rest, fs::remove),
            "editfile" => editfile(rest),
            _ => run_program(command, rest),
        };
        if let Err(err) = result {
            println!("{}: {}", command, err);
        }
    }
}

/// Anything that is not a builtin is looked up as a program.
fn run_program(name: &str, args: &[&str]) -> Result<(), Error> {
    match process::exec(name, args) {
        Ok(Exit::Success) => {}
        Ok(Exit::Interrupted) => println!("^C"),
        Ok(Exit::Error) => println!("{}: failed", name),
        Err(Error::NotFound) => println!("unknown command: {}", name),
        Err(err) => return Err(err),
    }
    Ok(())
}

fn with_name(
    command: &str,
    args: &[&str],
    f: impl FnOnce(&str) -> Result<(), Error>,
) -> Result<(), Error> {
    match args {
        [name] => f(name),
        _ => {
            println!("usage: {} <file>", command);
            Ok(())
        }
    }
}

fn ls() {
    let mut index = 0;
    while let Some(info) = fs::entry_at(index) {
        println!("{:>8}  {}", info.size, info.name.as_str());
        index += 1;
    }
    if index == 0 {
        println!("(no files)");
    }
}

fn df() -> Result<(), Error> {
    let s = fs::stats()?;
    let used = s.total_blocks - s.free_blocks;
    println!(
        "{} files, blocks: {} used / {} free / {} total ({} bytes each)",
        s.files, used, s.free_blocks, s.total_blocks, s.block_size
    );
    Ok(())
}

fn cat(name: &str) -> Result<(), Error> {
    let mut file = File::open(name, Mode::Read)?;
    let mut buf = [0u8; 64];
    let mut last = b'\n';
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        io::write_stdout(&buf[..n]);
        last = buf[n - 1];
    }
    if last != b'\n' {
        println!();
    }
    Ok(())
}

const EDITFILE_USAGE: &str = "usage: editfile <file> (addline <text> | write <text> | clear)";

/// Changes an existing file; never creates one (that is what `create` is for).
fn editfile(args: &[&str]) -> Result<(), Error> {
    let (name, action, words) = match args {
        [name, action, words @ ..] => (*name, *action, words),
        _ => {
            println!("{}", EDITFILE_USAGE);
            return Ok(());
        }
    };
    let mode = match (action, words) {
        ("addline", _) => Mode::Append,
        ("write", [_, ..]) => Mode::Write,
        ("clear", []) => Mode::Write,
        _ => {
            println!("{}", EDITFILE_USAGE);
            return Ok(());
        }
    };
    let mut file = File::open(name, mode)?;
    if action != "clear" {
        let mut text = words.join(" ");
        text.push('\n');
        file.write(text.as_bytes())?;
    }
    Ok(())
}

fn help() {
    println!("help         show this message");
    println!("echo <text>  print text");
    println!("clear        clear the screen");
    println!("exit         exit the shell (same as Ctrl+D)");
    println!("ls           list files");
    println!("df           file system usage");
    println!("create <f>   create an empty file");
    println!("cat <f>      print file");
    println!("rm <f>       delete file");
    println!("editfile <f> addline <text>      append a line to the file");
    println!("editfile <f> write <text>        replace file contents with the text");
    println!("editfile <f> clear               make the file empty");
    print!("programs:   ");
    let mut index = 0;
    while let Some(name) = process::program_at(index) {
        if name.as_str() != "shell" {
            print!(" {}", name.as_str());
        }
        index += 1;
    }
    println!();
    println!("Ctrl+C cancels the current line.");
}
