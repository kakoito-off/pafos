//! The contract between the kernel and programs: system call numbers, error
//! codes and the plain-old-data types that cross the boundary.
//!
//! This is the only crate both sides share. Everything here is a plain
//! integer or `#[repr(C)]`, so it means the same on both sides of the
//! ring 3 / ring 0 boundary.

#![no_std]

use core::fmt;

/// Interrupt vector of a system call.
///
/// Registers: `rax` = call number, `rdi`, `rsi`, `rdx`, `r10` = arguments
/// (as in Linux, so `rcx`/`r11` stay free for a future `syscall` instruction).
/// The result comes back in `rax`; every other register is preserved.
pub const SYSCALL_VECTOR: u8 = 0x80;

/// System call numbers.
///
/// Every call takes up to four `usize` arguments; strings and buffers are
/// passed as a (pointer, length) pair. The result is a non-negative value or
/// a negated [`Error`] code, see [`encode`] / [`decode`].
pub mod nr {
    /// `(flags)` → key as `u32`. With [`super::READ_KEY_NONBLOCK`] returns
    /// `WouldBlock` instead of waiting.
    pub const READ_KEY: usize = 1;
    /// `()` → 1 if Ctrl+C was pressed since the flag was last cleared.
    pub const STOP_REQUESTED: usize = 2;
    /// `()` → 0. Clears the Ctrl+C flag.
    pub const CLEAR_STOP: usize = 3;
    /// `(op)` → 0. See [`super::console`].
    pub const CONSOLE_CTL: usize = 4;

    /// `(name, name_len)` → 0. Fails with `AlreadyExists` if the name is taken.
    pub const CREATE: usize = 10;
    /// `(name, name_len, mode)` → fd. Never creates the file.
    pub const OPEN: usize = 11;
    /// `(fd, buf, len)` → bytes read, 0 at end of file.
    pub const READ: usize = 12;
    /// `(fd, buf, len)` → bytes written. [`super::STDOUT`] is the console.
    pub const WRITE: usize = 13;
    /// `(fd)` → 0.
    pub const CLOSE: usize = 14;
    /// `(name, name_len)` → 0.
    pub const UNLINK: usize = 15;
    /// `(index, *mut FileInfo)` → 1 if filled, 0 past the last file.
    pub const READDIR: usize = 16;
    /// `(*mut FsStats)` → 0.
    pub const FSSTAT: usize = 17;

    /// `()` → milliseconds since boot.
    pub const UPTIME_MS: usize = 20;

    /// `(name, name_len, argv: *const StrRef, argc)` → the program's [`super::Exit`] code.
    pub const EXEC: usize = 30;
    /// `(index, *mut Name)` → 1 if filled, 0 past the last program.
    pub const PROGRAM_AT: usize = 31;
    /// `(exit code)` → does not return. Only for programs running in ring 3.
    pub const EXIT: usize = 32;
}

/// Where a ring 3 program lives in its address space. Program ELF files are
/// linked to load at [`USER_BASE`]; the kernel puts the stack just below
/// [`USER_STACK_TOP`] and the arguments on top of it.
///
/// At entry `rdi` = argc and `rsi` = pointer to `argc` [`StrRef`]s.
pub mod layout {
    /// Start of the user region: exactly one level 4 page table entry (512 GiB).
    pub const USER_BASE: u64 = 0x6000_0000_0000;
    pub const USER_END: u64 = USER_BASE + (1 << 39);
    pub const USER_STACK_TOP: u64 = USER_BASE + 0x4000_0000;
    pub const USER_STACK_SIZE: u64 = 64 * 1024;
}

/// Most arguments a program can be started with.
pub const MAX_ARGS: usize = 32;

/// How many ring 3 programs can be nested (the shell starting a program
/// that starts a program, ...). Each level keeps its kernel frames on the
/// one kernel stack while the program above it runs.
pub const MAX_NESTING: usize = 8;

pub const READ_KEY_NONBLOCK: usize = 1;

/// Operations for [`nr::CONSOLE_CTL`].
pub mod console {
    /// Erase the last character on the current line.
    pub const BACKSPACE: usize = 1;
    pub const CLEAR_SCREEN: usize = 2;
}

/// Control keys as delivered by [`nr::READ_KEY`].
pub mod keys {
    pub const CTRL_C: char = '\u{3}';
    pub const CTRL_D: char = '\u{4}';
    pub const BACKSPACE: char = '\u{8}';
}

/// The console; writing to it prints on the screen.
pub const STDOUT: usize = 1;
/// File descriptors returned by [`nr::OPEN`] start here.
pub const FIRST_FILE_FD: usize = 3;

pub const MAX_NAME_LEN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(isize)]
pub enum Error {
    Unavailable = 1,
    NotFound = 2,
    AlreadyExists = 3,
    InvalidName = 4,
    TooManyFiles = 5,
    TooManyOpen = 6,
    Busy = 7,
    NoSpace = 8,
    BadFd = 9,
    WrongMode = 10,
    InvalidArgument = 11,
    WouldBlock = 12,
    NoSyscall = 13,
    TooDeep = 14,
}

impl Error {
    const ALL: [Error; 14] = [
        Error::Unavailable,
        Error::NotFound,
        Error::AlreadyExists,
        Error::InvalidName,
        Error::TooManyFiles,
        Error::TooManyOpen,
        Error::Busy,
        Error::NoSpace,
        Error::BadFd,
        Error::WrongMode,
        Error::InvalidArgument,
        Error::WouldBlock,
        Error::NoSyscall,
        Error::TooDeep,
    ];

    fn from_code(code: isize) -> Error {
        Error::ALL.into_iter().find(|&e| e as isize == code).unwrap_or(Error::InvalidArgument)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg = match self {
            Error::Unavailable => "file system not initialized",
            Error::NotFound => "not found",
            Error::AlreadyExists => "already exists",
            Error::InvalidName => "invalid name",
            Error::TooManyFiles => "too many files",
            Error::TooManyOpen => "too many open files",
            Error::Busy => "file is open",
            Error::NoSpace => "no space left",
            Error::BadFd => "bad file descriptor",
            Error::WrongMode => "file not opened in this mode",
            Error::InvalidArgument => "invalid argument",
            Error::WouldBlock => "would block",
            Error::NoSyscall => "no such system call",
            Error::TooDeep => "too many nested programs",
        };
        f.write_str(msg)
    }
}

/// Packs a system call result into the single register it is returned in.
pub fn encode(result: Result<usize, Error>) -> isize {
    match result {
        Ok(value) => value as isize,
        Err(err) => -(err as isize),
    }
}

pub fn decode(raw: isize) -> Result<usize, Error> {
    if raw < 0 { Err(Error::from_code(-raw)) } else { Ok(raw as usize) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum Mode {
    /// Read an existing file from the start.
    Read = 0,
    /// Truncate an existing file to zero length and write from the start.
    Write = 1,
    /// Write at the end of an existing file.
    Append = 2,
}

impl Mode {
    pub fn from_raw(raw: usize) -> Result<Mode, Error> {
        match raw {
            0 => Ok(Mode::Read),
            1 => Ok(Mode::Write),
            2 => Ok(Mode::Append),
            _ => Err(Error::InvalidArgument),
        }
    }
}

/// How a program finished; its `main` returns this, like `main`'s return
/// value in C.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum Exit {
    Success = 0,
    Error = 1,
    Interrupted = 2,
    /// Killed by the kernel: a CPU fault (bad memory access, invalid
    /// instruction, ...) or a panic.
    Crashed = 3,
}

impl Exit {
    pub fn from_raw(raw: usize) -> Exit {
        match raw {
            0 => Exit::Success,
            2 => Exit::Interrupted,
            3 => Exit::Crashed,
            _ => Exit::Error,
        }
    }
}

/// A fixed-capacity name (file or program), copied by value across the boundary.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Name {
    bytes: [u8; MAX_NAME_LEN],
    len: usize,
}

impl Name {
    pub const EMPTY: Name = Name { bytes: [0; MAX_NAME_LEN], len: 0 };

    /// Truncates names longer than [`MAX_NAME_LEN`].
    pub fn new(name: &str) -> Name {
        let len = name.len().min(MAX_NAME_LEN);
        let mut bytes = [0; MAX_NAME_LEN];
        bytes[..len].copy_from_slice(&name.as_bytes()[..len]);
        Name { bytes, len }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len.min(MAX_NAME_LEN)]).unwrap_or("?")
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct FileInfo {
    pub name: Name,
    pub size: usize,
}

impl FileInfo {
    pub const EMPTY: FileInfo = FileInfo { name: Name::EMPTY, size: 0 };
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct FsStats {
    pub block_size: usize,
    pub total_blocks: usize,
    pub free_blocks: usize,
    pub files: usize,
}

/// One element of the argument vector passed to [`nr::EXEC`].
#[derive(Clone, Copy)]
#[repr(C)]
pub struct StrRef {
    pub ptr: *const u8,
    pub len: usize,
}

impl StrRef {
    pub fn new(s: &str) -> StrRef {
        StrRef { ptr: s.as_ptr(), len: s.len() }
    }
}
