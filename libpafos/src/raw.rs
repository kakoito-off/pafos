//! The one place where a program crosses into the kernel: `int 0x80`.
//!
//! Register convention: see `pafos_abi::SYSCALL_VECTOR`.

use core::arch::asm;
use pafos_abi::{Error, SYSCALL_VECTOR, decode};

/// # Safety
/// Pointer arguments must be valid for what the call `nr` does with them
/// (see `pafos_abi::nr`).
pub unsafe fn syscall(nr: usize, args: [usize; 4]) -> Result<usize, Error> {
    let ret: isize;
    // No `nostack`: in ring 0 the CPU pushes the interrupt frame onto the
    // current stack. No `nomem`: the kernel reads and writes our buffers.
    unsafe {
        asm!(
            "int {vector}",
            vector = const SYSCALL_VECTOR,
            inlateout("rax") nr as isize => ret,
            in("rdi") args[0],
            in("rsi") args[1],
            in("rdx") args[2],
            in("r10") args[3],
        );
    }
    decode(ret)
}

/// For calls whose arguments are plain numbers, so nothing can go wrong.
pub(crate) fn syscall_noptr(nr: usize, args: [usize; 4]) -> Result<usize, Error> {
    unsafe { syscall(nr, args) }
}
