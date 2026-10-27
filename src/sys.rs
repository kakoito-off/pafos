//! Facade between programs (shell, calculator, ...) and the kernel.
//!
//! Programs talk to the kernel only through this module. Today everything runs
//! in ring 0 and these are plain function calls; when programs move to ring 3,
//! the same functions become system calls and program code stays unchanged.

use crate::input;
use crate::vga_buffer::WRITER;
use x86_64::instructions::interrupts;

pub use crate::input::{CTRL_C, CTRL_D};
pub use crate::{print, println};

pub const BACKSPACE: char = '\u{8}';

/// How a program finished. Returned from the program's entry point, like
/// `main`'s return value in C: in ring 3 the program's runtime would hand
/// it to an `exit` system call instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Success,
    Interrupted,
    Error,
}

/// Entry point signature every program implements.
pub type Program = fn(args: &[&str]) -> Exit;

/// Starts a program and waits for it to finish.
pub fn run(program: Program, args: &[&str]) -> Exit {
    input::clear_stop();
    let exit = program(args);
    input::clear_stop();
    exit
}

/// Blocks until a key is pressed.
pub fn read_char() -> char {
    input::pop_blocking()
}

/// Returns a key if one is waiting, without blocking.
pub fn try_read_char() -> Option<char> {
    input::try_pop()
}

/// True once Ctrl+C was pressed; long-running programs should poll this
/// and return `Exit::Interrupted` when it is set.
pub fn should_stop() -> bool {
    input::stop_requested()
}

pub fn clear_stop() {
    input::clear_stop();
}

pub fn backspace() {
    interrupts::without_interrupts(|| WRITER.lock().backspace());
}

pub fn clear_screen() {
    interrupts::without_interrupts(|| WRITER.lock().clear_screen());
}
