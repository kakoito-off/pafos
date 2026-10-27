//! User programs. Code in here may only depend on `crate::sys`, never on
//! kernel internals, so it can later be split out and run in ring 3.

pub mod shell;
