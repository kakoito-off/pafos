//! Running other programs.

use crate::raw::syscall;
use alloc::vec::Vec;
use pafos_abi::{Error, Exit, Name, StrRef, nr};

/// Runs the program `name` and waits for it to finish.
pub fn exec(name: &str, args: &[&str]) -> Result<Exit, Error> {
    let argv: Vec<StrRef> = args.iter().map(|arg| StrRef::new(arg)).collect();
    let code = unsafe {
        syscall(
            nr::EXEC,
            [name.as_ptr() as usize, name.len(), argv.as_ptr() as usize, argv.len()],
        )?
    };
    Ok(Exit::from_raw(code))
}

/// The `index`-th installed program, or `None` past the last one.
pub fn program_at(index: usize) -> Option<Name> {
    let mut name = Name::EMPTY;
    let filled = unsafe { syscall(nr::PROGRAM_AT, [index, &raw mut name as usize, 0, 0]) };
    (filled == Ok(1)).then_some(name)
}
