//! Installed programs.
//!
//! Each program is its own crate under `programs/` that depends only on
//! `libpafos`, so it cannot touch kernel internals. Two kinds for now:
//! - linked into the kernel and started by a plain call, still in ring 0
//!   (`shell`, `calc`, `timer`);
//! - separate ELF executables built by `build.rs`, embedded here and run in
//!   ring 3 in their own address space (`time`, `fault`).

use crate::{input, process};
use pafos_abi::{Error, Exit, Name};

type Main = fn(args: &[&str]) -> Exit;

// Unused variants and macro in builds without (some) programs.
#[allow(dead_code)]
enum Program {
    Linked(Main),
    Executable(&'static [u8]),
}

/// An executable built by build.rs.
#[allow(unused_macros)]
macro_rules! executable {
    ($name:literal) => {
        Program::Executable(include_bytes!(concat!(env!("OUT_DIR"), "/", $name)))
    };
}

static PROGRAMS: &[(&str, Program)] = &[
    #[cfg(feature = "shell")]
    ("shell", Program::Linked(pafos_shell::main)),
    #[cfg(feature = "calc")]
    ("calc", Program::Linked(pafos_calc::main)),
    #[cfg(feature = "time")]
    ("time", executable!("time")),
    #[cfg(feature = "timer")]
    ("timer", Program::Linked(pafos_timer::main)),
    #[cfg(feature = "fault")]
    ("fault", executable!("fault")),
];

/// Starts a program and waits for it to finish.
pub fn run(name: &str, args: &[&str]) -> Result<Exit, Error> {
    let (_, program) = PROGRAMS.iter().find(|(n, _)| *n == name).ok_or(Error::NotFound)?;
    input::clear_stop();
    let exit = match program {
        Program::Linked(main) => Ok(main(args)),
        Program::Executable(image) => process::run(name, image, args),
    };
    input::clear_stop();
    exit
}

pub fn name_at(index: usize) -> Option<Name> {
    PROGRAMS.get(index).map(|(name, _)| Name::new(name))
}
