//! Runtime for programs that run as their own ring 3 executable.
//!
//! A program crate builds a binary with `#![no_std]`, `#![no_main]` and
//! `libpafos::entry!(main);`. The kernel jumps to `_start` with the
//! arguments on the stack (see `pafos_abi::layout`); [`start`] sets up the
//! heap, calls `main` and passes its result to [`exit`].

use crate::raw::syscall_noptr;
use crate::{Exit, Main, println};
use core::panic::PanicInfo;
use linked_list_allocator::LockedHeap;
use pafos_abi::{MAX_ARGS, StrRef, nr};

pub use pafos_abi::StrRef as RawArg;

/// Defines the program's entry point: `libpafos::entry!(main);`
#[macro_export]
macro_rules! entry {
    ($main:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn _start(argc: usize, argv: *const $crate::rt::RawArg) -> ! {
            $crate::rt::start(argc, argv, $main)
        }
    };
}

/// Ends the program.
pub fn exit(code: Exit) -> ! {
    let _ = syscall_noptr(nr::EXIT, [code as usize, 0, 0, 0]);
    // The kernel never returns from EXIT for a ring 3 program.
    loop {}
}

const HEAP_SIZE: usize = 64 * 1024;
// Part of the program image (.bss), so the kernel maps it like any other
// data; no system call is needed to get memory.
static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

#[doc(hidden)]
pub fn start(argc: usize, argv: *const StrRef, main: Main) -> ! {
    unsafe { ALLOCATOR.lock().init(&raw mut HEAP as usize, HEAP_SIZE) };

    let mut args = [""; MAX_ARGS];
    let argc = argc.min(MAX_ARGS);
    for (i, arg) in args.iter_mut().enumerate().take(argc) {
        // The kernel copied valid UTF-8 strings onto our stack.
        *arg = unsafe {
            let raw = *argv.add(i);
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(raw.ptr, raw.len))
        };
    }
    exit(main(&args[..argc]))
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // Prints as "panicked at <file>:<line>:<col>:\n<message>".
    println!("{}", info);
    exit(Exit::Crashed)
}
