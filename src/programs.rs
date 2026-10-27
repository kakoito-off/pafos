//! Installed programs.
//!
//! Each program is its own crate under `programs/` that depends only on
//! `libpafos`, so it cannot touch kernel internals. `build.rs` builds each
//! one into an ELF executable, embedded here, and `process::run` starts it in
//! ring 3 in its own address space.

use crate::{input, process};
use pafos_abi::{Error, Exit, Name};

/// The ELF file of a program built by build.rs.
#[allow(unused_macros)] // in builds without programs
macro_rules! executable {
    ($name:literal) => {
        include_bytes!(concat!(env!("OUT_DIR"), "/", $name))
    };
}

static PROGRAMS: &[(&str, &[u8])] = &[
    #[cfg(feature = "shell")]
    ("shell", executable!("shell")),
    #[cfg(feature = "calc")]
    ("calc", executable!("calc")),
    #[cfg(feature = "time")]
    ("time", executable!("time")),
    #[cfg(feature = "timer")]
    ("timer", executable!("timer")),
    #[cfg(feature = "fault")]
    ("fault", executable!("fault")),
];

/// Starts a program and waits for it to finish.
pub fn run(name: &str, args: &[&str]) -> Result<Exit, Error> {
    let (_, image) = PROGRAMS.iter().find(|(n, _)| *n == name).ok_or(Error::NotFound)?;
    input::clear_stop();
    let exit = process::run(name, image, args);
    input::clear_stop();
    exit
}

pub fn name_at(index: usize) -> Option<Name> {
    PROGRAMS.get(index).map(|(name, _)| Name::new(name))
}
