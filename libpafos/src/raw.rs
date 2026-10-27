//! The one place where a program crosses into the kernel.
//!
//! Today programs run in ring 0 and this is a plain call of a kernel
//! function. With ring 3 only the body of [`syscall`] changes: the number
//! and arguments go into registers and `int 0x80` traps into the kernel's
//! dispatcher. Nothing above this module notices.

use pafos_abi::{Error, decode};

unsafe extern "C" {
    /// Defined by the kernel (`syscall.rs`).
    fn pafos_syscall(nr: usize, a0: usize, a1: usize, a2: usize, a3: usize) -> isize;
}

/// # Safety
/// Pointer arguments must be valid for what the call `nr` does with them
/// (see `pafos_abi::nr`).
pub unsafe fn syscall(nr: usize, args: [usize; 4]) -> Result<usize, Error> {
    decode(unsafe { pafos_syscall(nr, args[0], args[1], args[2], args[3]) })
}

/// For calls whose arguments are plain numbers, so nothing can go wrong.
pub(crate) fn syscall_noptr(nr: usize, args: [usize; 4]) -> Result<usize, Error> {
    unsafe { syscall(nr, args) }
}
