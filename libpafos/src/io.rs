//! Console input and output.

use crate::raw::{syscall, syscall_noptr};
use alloc::string::String;
use core::fmt;
use pafos_abi::{READ_KEY_NONBLOCK, STDOUT, console, nr};

pub use pafos_abi::keys::{BACKSPACE, CTRL_C, CTRL_D};

/// Writes raw bytes to the console.
pub fn write_stdout(bytes: &[u8]) {
    // Printing has nowhere to report a failure to, same as `print!` in std.
    let _ = unsafe { syscall(nr::WRITE, [STDOUT, bytes.as_ptr() as usize, bytes.len(), 0]) };
}

struct Stdout;

impl fmt::Write for Stdout {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        write_stdout(s.as_bytes());
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    let _ = Stdout.write_fmt(args);
}

fn to_char(raw: usize) -> char {
    char::from_u32(raw as u32).unwrap_or('\0')
}

/// Blocks until a key is pressed.
pub fn read_key() -> char {
    syscall_noptr(nr::READ_KEY, [0; 4]).map_or('\0', to_char)
}

/// Returns a key if one is waiting, without blocking.
pub fn try_read_key() -> Option<char> {
    syscall_noptr(nr::READ_KEY, [READ_KEY_NONBLOCK, 0, 0, 0]).ok().map(to_char)
}

/// True once Ctrl+C was pressed; long-running programs should poll this
/// and return `Exit::Interrupted` when it is set.
pub fn should_stop() -> bool {
    syscall_noptr(nr::STOP_REQUESTED, [0; 4]) == Ok(1)
}

pub fn clear_stop() {
    let _ = syscall_noptr(nr::CLEAR_STOP, [0; 4]);
}

pub fn backspace() {
    let _ = syscall_noptr(nr::CONSOLE_CTL, [console::BACKSPACE, 0, 0, 0]);
}

pub fn clear_screen() {
    let _ = syscall_noptr(nr::CONSOLE_CTL, [console::CLEAR_SCREEN, 0, 0, 0]);
}

pub enum LineEnd {
    Enter,
    /// Ctrl+C; the line is discarded and the stop flag cleared.
    Cancel,
    /// Ctrl+D.
    Eof,
}

/// Reads a line with echo and backspace into `line` (cleared first),
/// accepting at most `max_len` printable ASCII characters.
pub fn read_line(line: &mut String, max_len: usize) -> LineEnd {
    line.clear();
    loop {
        match read_key() {
            '\n' => {
                crate::println!();
                return LineEnd::Enter;
            }
            CTRL_D => {
                crate::println!();
                return LineEnd::Eof;
            }
            CTRL_C => {
                crate::println!("^C");
                clear_stop();
                return LineEnd::Cancel;
            }
            BACKSPACE => {
                if line.pop().is_some() {
                    backspace();
                }
            }
            c if c.is_ascii_graphic() || c == ' ' => {
                if line.len() < max_len {
                    line.push(c);
                    crate::print!("{}", c);
                }
            }
            _ => {}
        }
    }
}
