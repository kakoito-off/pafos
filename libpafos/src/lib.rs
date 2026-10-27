//! The library every PAFOS program links against (think of it as a tiny libc).
//!
//! Programs depend on this crate only, never on the kernel. All of it is
//! built on [`raw::syscall`]; conveniences (formatting, line editing, `File`
//! with `Drop`, ...) live here, so the kernel's system call set stays small.

#![no_std]

extern crate alloc;

pub mod fs;
pub mod io;
pub mod process;
pub mod raw;
pub mod time;

pub use pafos_abi::{Error, Exit, MAX_NAME_LEN};

/// Entry point signature every program implements.
pub type Main = fn(args: &[&str]) -> Exit;

/// Like `std::print!`: formats and writes to the console.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::io::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}
