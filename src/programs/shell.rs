use crate::sys::{self, Exit, File, FsError, Mode, print, println};
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
                sys::clear_screen();
                Ok(())
            }
            "ls" => {
                ls();
                Ok(())
            }
            "df" => df(),
            "cat" => with_name(command, rest, cat),
            "touch" => with_name(command, rest, |name| File::open(name, Mode::Append).map(drop)),
            "rm" => with_name(command, rest, sys::remove_file),
            "write" => write_text(command, rest, Mode::Write),
            "append" => write_text(command, rest, Mode::Append),
            _ => {
                println!("unknown command: {}", command);
                Ok(())
            }
        };
        if let Err(err) = result {
            println!("{}: {}", command, err);
        }
    }
}

fn with_name(
    command: &str,
    args: &[&str],
    f: impl FnOnce(&str) -> Result<(), FsError>,
) -> Result<(), FsError> {
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
    while let Some(info) = sys::file_at(index) {
        println!("{:>8}  {}", info.size, info.name());
        index += 1;
    }
    if index == 0 {
        println!("(no files)");
    }
}

fn df() -> Result<(), FsError> {
    let s = sys::fs_stats()?;
    let used = s.total_blocks - s.free_blocks;
    println!(
        "{} files, blocks: {} used / {} free / {} total ({} bytes each)",
        s.files, used, s.free_blocks, s.total_blocks, s.block_size
    );
    Ok(())
}

fn cat(name: &str) -> Result<(), FsError> {
    let mut file = File::open(name, Mode::Read)?;
    let mut buf = [0u8; 64];
    let mut last = b'\n';
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        for &byte in &buf[..n] {
            print!("{}", byte as char);
        }
        last = buf[n - 1];
    }
    if last != b'\n' {
        println!();
    }
    Ok(())
}

fn write_text(command: &str, args: &[&str], mode: Mode) -> Result<(), FsError> {
    let Some((&name, words)) = args.split_first() else {
        println!("usage: {} <file> <text...>", command);
        return Ok(());
    };
    let mut text = words.join(" ");
    text.push('\n');
    File::open(name, mode)?.write(text.as_bytes())?;
    Ok(())
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
    println!("ls           list files          df            file system usage");
    println!("cat <f>      print file          touch <f>     create empty file");
    println!("write <f> <text>   overwrite     append <f> <text>   add a line");
    println!("rm <f>       delete file");
    println!("Ctrl+C cancels the current line.");
}
