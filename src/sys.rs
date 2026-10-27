//! Facade between programs (shell, calculator, ...) and the kernel.
//!
//! Programs talk to the kernel only through this module. Today everything runs
//! in ring 0 and these are plain function calls; when programs move to ring 3,
//! the same functions become system calls and program code stays unchanged.

use crate::input;
use crate::tmpfs;
use crate::vga_buffer::WRITER;
use x86_64::instructions::interrupts;

pub use crate::input::{CTRL_C, CTRL_D};
pub use crate::tmpfs::{FileInfo, FsError, FsStats, MAX_NAME_LEN, Mode};
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

/// An open file. Data is always copied between the kernel and the caller's
/// buffers, never handed out by reference, so the same API works across a
/// future ring 3 boundary. Closed automatically when dropped.
pub struct File {
    fd: usize,
}

impl File {
    pub fn open(name: &str, mode: Mode) -> Result<File, FsError> {
        tmpfs::open(name, mode).map(|fd| File { fd })
    }

    /// Reads up to `buf.len()` bytes; returns 0 at end of file.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, FsError> {
        tmpfs::read(self.fd, buf)
    }

    pub fn write(&mut self, data: &[u8]) -> Result<usize, FsError> {
        tmpfs::write(self.fd, data)
    }
}

impl Drop for File {
    fn drop(&mut self) {
        let _ = tmpfs::close(self.fd);
    }
}

pub fn remove_file(name: &str) -> Result<(), FsError> {
    tmpfs::remove(name)
}

/// The `index`-th file in name order, or `None` past the last one.
pub fn file_at(index: usize) -> Option<FileInfo> {
    tmpfs::file_at(index)
}

pub fn fs_stats() -> Result<FsStats, FsError> {
    tmpfs::stats()
}
