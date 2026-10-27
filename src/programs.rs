//! Installed programs.
//!
//! Each program is its own crate under `programs/` that depends only on
//! `libpafos`, so it cannot touch kernel internals. For now they are linked
//! into the kernel image and started by a plain function call; with ring 3
//! this table becomes a lookup of an executable (an ELF image) that `run`
//! loads into a fresh address space.

use crate::input;
use pafos_abi::{Error, Exit, Name};

type Main = fn(args: &[&str]) -> Exit;

static PROGRAMS: &[(&str, Main)] = &[
    #[cfg(feature = "shell")]
    ("shell", pafos_shell::main),
    #[cfg(feature = "calc")]
    ("calc", pafos_calc::main),
    #[cfg(feature = "time")]
    ("time", pafos_time::main),
    #[cfg(feature = "timer")]
    ("timer", pafos_timer::main),
];

/// Starts a program and waits for it to finish.
pub fn run(name: &str, args: &[&str]) -> Result<Exit, Error> {
    let &(_, main) = PROGRAMS.iter().find(|(n, _)| *n == name).ok_or(Error::NotFound)?;
    input::clear_stop();
    let exit = main(args);
    input::clear_stop();
    Ok(exit)
}

pub fn name_at(index: usize) -> Option<Name> {
    PROGRAMS.get(index).map(|(name, _)| Name::new(name))
}
